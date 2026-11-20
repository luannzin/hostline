//! One visitor: the WebRTC answer, then every request and WebSocket the visitor's page makes,
//! carried over one data channel and sent on to the port being shared.
//!
//! Every message on the channel is a frame: one byte saying what it is, four bytes naming the
//! request or socket it belongs to, then its payload. `PROTOCOL.md` lists them.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::{BufMut, BytesMut};
use futures_util::{SinkExt, StreamExt};
use reqwest::cookie::{CookieStore, Jar};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message as Ws;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceGatheringState, RTCIceServer, RTCPeerConnectionState, RTCSessionDescription,
};

// visitor to host
const REQ: u8 = 1;
const REQ_BODY: u8 = 2;
const REQ_END: u8 = 3;
const CANCEL: u8 = 4;
const WS_OPEN: u8 = 8;
// host to visitor
const RES: u8 = 5;
const RES_BODY: u8 = 6;
const RES_END: u8 = 7;
const WS_OPENED: u8 = 9;
const FAIL: u8 = 13;
const BYE: u8 = 14;
// both ways
const WS_TEXT: u8 = 10;
const WS_BIN: u8 = 11;
const WS_CLOSE: u8 = 12;

/// Under every browser's data channel message limit.
const CHUNK: usize = 16 * 1024;

const STUN: [&str; 2] = ["stun:stun.l.google.com:19302", "stun:stun.cloudflare.com:3478"];

/// What's being shared: `127.0.0.1:5173`, `[::1]:3000`.
pub struct Target {
    pub authority: String,
}

impl Target {
    fn http(&self) -> String {
        format!("http://{}", self.authority)
    }
}

pub enum Note {
    Joined(String),
    Left(String),
}

/// Answers one visitor's offer. Returns the connection and the answer, gathered whole (one
/// signalling message each way, no trickle).
pub async fn answer(
    offer: String,
    who: String,
    target: Arc<Target>,
    notes: mpsc::UnboundedSender<Note>,
    ending: watch::Receiver<bool>,
) -> Result<(Arc<dyn PeerConnection>, String)> {
    let (gathered_tx, mut gathered) = mpsc::channel(1);
    let config = RTCConfigurationBuilder::new()
        .with_ice_servers(vec![RTCIceServer {
            urls: STUN.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }])
        .build();
    let pc: Arc<dyn PeerConnection> = Arc::new(
        PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_handler(Arc::new(Handler {
                gathered: gathered_tx,
                who,
                target,
                notes,
                ending,
            }))
            .with_udp_addrs(vec!["0.0.0.0:0", "[::]:0"])
            .build()
            .await?,
    );
    pc.set_remote_description(RTCSessionDescription::offer(offer)?).await?;
    let local = pc.create_answer(None).await?;
    pc.set_local_description(local).await?;
    let _ = tokio::time::timeout(Duration::from_secs(5), gathered.recv()).await;
    let sdp = pc.local_description().await.context("no answer")?.sdp;
    Ok((pc, sdp))
}

struct Handler {
    gathered: mpsc::Sender<()>,
    who: String,
    target: Arc<Target>,
    notes: mpsc::UnboundedSender<Note>,
    ending: watch::Receiver<bool>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered.try_send(());
        }
    }

    async fn on_data_channel(&self, dc: Arc<dyn DataChannel>) {
        let visit = Visit {
            dc,
            target: self.target.clone(),
            // one cookie jar per visitor, kept on this machine
            jar: Arc::new(Jar::default()),
        };
        let (who, notes, ending) = (self.who.clone(), self.notes.clone(), self.ending.clone());
        tokio::spawn(async move {
            let _ = notes.send(Note::Joined(who.clone()));
            visit.serve(ending).await;
            let _ = notes.send(Note::Left(who));
        });
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
            let _ = self.gathered.try_send(());
        }
    }
}

#[derive(Deserialize)]
struct ReqHead {
    m: String,
    u: String,
    h: Vec<(String, String)>,
}

#[derive(Serialize)]
struct ResHead {
    s: u16,
    t: String,
    h: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct WsOpen {
    u: String,
    p: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct WsClose {
    c: u16,
    r: String,
}

enum Stream {
    Req(ReqHead, Vec<u8>),
    Task(JoinHandle<()>),
    Ws(mpsc::UnboundedSender<Ws>, JoinHandle<()>),
}

#[derive(Clone)]
struct Visit {
    dc: Arc<dyn DataChannel>,
    target: Arc<Target>,
    jar: Arc<Jar>,
}

impl Visit {
    async fn serve(self, mut ending: watch::Receiver<bool>) {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .cookie_provider(self.jar.clone())
            .build()
            .expect("http client");
        let mut streams: HashMap<u32, Stream> = HashMap::new();
        loop {
            let event = tokio::select! {
                event = self.dc.poll() => event,
                _ = ended(&mut ending) => {
                    // no close here: closing right away can drop the goodbye before it leaves
                    let _ = self.frame(BYE, 0, &[]).await;
                    return;
                }
            };
            let Some(event) = event else { break };
            let DataChannelEvent::OnMessage(msg) = event else {
                if matches!(event, DataChannelEvent::OnClose) {
                    break;
                }
                continue;
            };
            let Some((kind, id, payload)) = split(&msg.data) else { continue };
            match kind {
                REQ => {
                    if let Ok(head) = serde_json::from_slice(payload) {
                        streams.retain(|_, s| match s {
                            Stream::Task(t) | Stream::Ws(_, t) => !t.is_finished(),
                            Stream::Req(..) => true,
                        });
                        streams.insert(id, Stream::Req(head, Vec::new()));
                    }
                }
                REQ_BODY => {
                    if let Some(Stream::Req(_, body)) = streams.get_mut(&id) {
                        body.extend_from_slice(payload);
                    }
                }
                REQ_END => {
                    if let Some(Stream::Req(head, body)) = streams.remove(&id) {
                        let visit = self.clone();
                        let http = http.clone();
                        let task = tokio::spawn(async move {
                            if let Err(e) = visit.forward(&http, id, head, body).await {
                                let _ = visit.frame(FAIL, id, format!("{e:#}").as_bytes()).await;
                            }
                        });
                        streams.insert(id, Stream::Task(task));
                    }
                }
                CANCEL => match streams.remove(&id) {
                    Some(Stream::Task(t)) | Some(Stream::Ws(_, t)) => t.abort(),
                    _ => {}
                },
                WS_OPEN => {
                    if let Ok(open) = serde_json::from_slice::<WsOpen>(payload) {
                        let (tx, rx) = mpsc::unbounded_channel();
                        let visit = self.clone();
                        let task = tokio::spawn(async move {
                            if let Err(e) = visit.socket(id, open, rx).await {
                                let close = WsClose { c: 1006, r: format!("{e:#}") };
                                let json = serde_json::to_vec(&close).unwrap_or_default();
                                let _ = visit.frame(WS_CLOSE, id, &json).await;
                            }
                        });
                        streams.insert(id, Stream::Ws(tx, task));
                    }
                }
                WS_TEXT | WS_BIN | WS_CLOSE => {
                    if let Some(Stream::Ws(tx, _)) = streams.get(&id) {
                        let msg = match kind {
                            WS_TEXT => Ws::Text(String::from_utf8_lossy(payload).into_owned().into()),
                            WS_BIN => Ws::Binary(payload.to_vec().into()),
                            _ => {
                                let c: WsClose = serde_json::from_slice(payload)
                                    .unwrap_or(WsClose { c: 1000, r: String::new() });
                                Ws::Close(Some(CloseFrame { code: c.c.into(), reason: c.r.into() }))
                            }
                        };
                        let _ = tx.send(msg);
                    }
                }
                _ => {}
            }
        }
        for (_, stream) in streams {
            if let Stream::Task(t) | Stream::Ws(_, t) = stream {
                t.abort();
            }
        }
        let _ = self.dc.close().await;
    }

    async fn frame(&self, kind: u8, id: u32, payload: &[u8]) -> Result<()> {
        let mut buf = BytesMut::with_capacity(5 + payload.len());
        buf.put_u8(kind);
        buf.put_u32(id);
        buf.extend_from_slice(payload);
        self.dc.send(buf).await?;
        Ok(())
    }

    async fn forward(&self, http: &reqwest::Client, id: u32, head: ReqHead, body: Vec<u8>) -> Result<()> {
        let method = reqwest::Method::from_bytes(head.m.as_bytes())?;
        let writes = !matches!(method, reqwest::Method::GET | reqwest::Method::HEAD);
        let mut req = http.request(method, format!("{}{}", self.target.http(), head.u));
        for (name, value) in head.h {
            let lower = name.to_ascii_lowercase();
            if SKIP_REQUEST.contains(&lower.as_str()) {
                continue;
            }
            req = req.header(name, if lower == "referer" { self.rebase(&value) } else { value });
        }
        // Frameworks compare Origin with Host before accepting a form or a Server Action.
        if writes {
            req = req.header("origin", self.target.http());
        }
        let res = req.body(body).send().await?;
        let head = ResHead {
            s: res.status().as_u16(),
            t: res.status().canonical_reason().unwrap_or("").into(),
            h: res
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    let name = name.as_str();
                    let value = value.to_str().ok()?;
                    match name {
                        _ if SKIP_RESPONSE.contains(&name) => None,
                        "location" => Some((name.into(), self.local_path(value))),
                        "content-security-policy" => {
                            let kept: Vec<&str> = value
                                .split(';')
                                .filter(|d| !d.trim_start().starts_with("frame-ancestors"))
                                .collect();
                            Some((name.into(), kept.join(";")))
                        }
                        _ => Some((name.into(), value.into())),
                    }
                })
                .collect(),
        };
        self.frame(RES, id, &serde_json::to_vec(&head)?).await?;
        let mut body = res.bytes_stream();
        while let Some(chunk) = body.next().await {
            for piece in chunk?.chunks(CHUNK) {
                self.frame(RES_BODY, id, piece).await?;
            }
        }
        self.frame(RES_END, id, &[]).await
    }

    async fn socket(&self, id: u32, open: WsOpen, mut from_page: mpsc::UnboundedReceiver<Ws>) -> Result<()> {
        // Always the shared port, whatever host the page asked for.
        let url = format!("ws://{}{}", self.target.authority, open.u);
        let mut request = url.as_str().into_client_request()?;
        let headers = request.headers_mut();
        if !open.p.is_empty() {
            headers.insert("sec-websocket-protocol", open.p.join(", ").parse()?);
        }
        headers.insert("origin", self.target.http().parse()?);
        let http_url = format!("{}{}", self.target.http(), open.u).parse()?;
        if let Some(cookies) = self.jar.cookies(&http_url) {
            headers.insert("cookie", cookies);
        }
        let (ws, response) = tokio_tungstenite::connect_async(request).await?;
        let protocol = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|p| p.to_str().ok())
            .unwrap_or("");
        self.frame(WS_OPENED, id, &serde_json::to_vec(&serde_json::json!({ "p": protocol }))?)
            .await?;
        let (mut sink, mut stream) = ws.split();
        loop {
            tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(Ws::Text(text))) => self.frame(WS_TEXT, id, text.as_bytes()).await?,
                    Some(Ok(Ws::Binary(data))) => {
                        // a WebSocket message is one data channel message, so no chunking here
                        self.frame(WS_BIN, id, &data).await?
                    }
                    Some(Ok(Ws::Close(frame))) => {
                        let (c, r) = frame.map(|f| (f.code.into(), f.reason.to_string())).unwrap_or((1000, String::new()));
                        self.frame(WS_CLOSE, id, &serde_json::to_vec(&WsClose { c, r })?).await?;
                        return Ok(());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => {
                        self.frame(WS_CLOSE, id, &serde_json::to_vec(&WsClose { c: 1006, r: String::new() })?).await?;
                        return Ok(());
                    }
                },
                msg = from_page.recv() => match msg {
                    Some(msg) => {
                        let closing = matches!(msg, Ws::Close(_));
                        sink.send(msg).await?;
                        if closing { return Ok(()); }
                    }
                    None => return Ok(()),
                },
            }
        }
    }

    /// `https://hostline.luannzin.com/a?b` becomes `http://127.0.0.1:5173/a?b`.
    fn rebase(&self, url: &str) -> String {
        match url.split_once("://").and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..])) {
            Some(path) => format!("{}{path}", self.target.http()),
            None => self.target.http(),
        }
    }

    /// A redirect to the shared port itself becomes a path, so the visitor stays on the link.
    fn local_path(&self, location: &str) -> String {
        let port = self.target.authority.rsplit(':').next().unwrap_or("");
        LOCAL_ORIGINS
            .iter()
            .find_map(|origin| location.strip_prefix(&format!("{origin}:{port}")))
            .map(|rest| if rest.is_empty() { "/".into() } else { rest.into() })
            .unwrap_or_else(|| location.into())
    }
}

const LOCAL_ORIGINS: [&str; 4] = ["http://localhost", "http://127.0.0.1", "http://[::1]", "http://0.0.0.0"];

/// Set by the connection to the shared port, or meaningless across it.
const SKIP_REQUEST: [&str; 12] = [
    "host", "connection", "keep-alive", "upgrade", "te", "trailer", "transfer-encoding",
    "content-length", "accept-encoding", "cookie", "origin", "proxy-authorization",
];

/// Cookies stay in this machine's jar; the body arrives decompressed and in pieces; and the
/// rest would apply to the link's own address instead of the app.
const SKIP_RESPONSE: [&str; 11] = [
    "set-cookie", "content-length", "content-encoding", "transfer-encoding", "connection",
    "keep-alive", "x-frame-options", "strict-transport-security", "clear-site-data", "alt-svc",
    "upgrade",
];

/// Resolves once the host says the link ended. (`wait_for` holds a lock guard across the await,
/// which keeps the task off other threads.)
async fn ended(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            return std::future::pending().await;
        }
    }
}

fn split(data: &[u8]) -> Option<(u8, u32, &[u8])> {
    let (head, payload) = data.split_at_checked(5)?;
    Some((head[0], u32::from_be_bytes(head[1..5].try_into().ok()?), payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frames() {
        assert_eq!(split(&[1, 0, 0, 1, 2, 9]), Some((1, 258, &[9][..])));
        assert_eq!(split(&[1, 0]), None);
    }
}
