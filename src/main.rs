//! Hostline: your localhost as a link.
//!
//! `hostline 5173` prints a link. Whoever opens it gets that port in their browser, loaded straight
//! from this machine over WebRTC. The link ends when this stops.

mod peer;
mod signal;
mod update;

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use futures_util::StreamExt;
use nostr_sdk::prelude::*;
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use tokio::sync::{mpsc, watch};

use peer::{Cookies, Note, Target};
use signal::{Message, Room};

const SHELL: &str = "https://hostline.luannzin.com/";

/// Where dev servers, previews and local servers usually listen, probed when no port is given.
const USUAL_PORTS: [u16; 16] = [
    3000, 5173, 4321, 8080, 8000, 4200, 5000, 3001, 4173, 5174, 8787, 1313, 4000, 8888, 9000, 8081,
];

#[tokio::main]
async fn main() {
    // Two TLS backends get pulled in (WebRTC's and the relays'); pick one for the whole process.
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Err(e) = run().await {
        eprintln!("hostline: {e:#}");
        std::process::exit(1);
    }
}

struct Args {
    target: Option<String>,
    duration: Option<Duration>,
}

fn args() -> Result<Args> {
    let mut out = Args { target: None, duration: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("hostline {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--for" => {
                let Some(value) = it.next() else { bail!("--for needs a time, like --for 30m") };
                out.duration = Some(duration(&value)?);
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}\n\n{HELP}"),
            _ => out.target = Some(arg),
        }
    }
    Ok(out)
}

const HELP: &str = "Your localhost as a link.

Usage: hostline [PORT | HOST:PORT] [--for TIME]

  hostline           share the one usual port that's listening (3000, 5173, 8080, ...)
  hostline 8080      share port 8080
  hostline --for 30m end the link after 30 minutes (s, m, h)

The link works while hostline runs. Stop it (Ctrl+C) and the link ends.";

/// `45s`, `30m`, `2h`, or a bare number of minutes.
fn duration(text: &str) -> Result<Duration> {
    let split = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("--for takes a time like 30m, not {text}"))?;
    let seconds = match unit {
        "s" => n,
        "" | "m" => n * 60,
        "h" => n * 3600,
        _ => bail!("--for takes s, m or h, not {text}"),
    };
    Ok(Duration::from_secs(seconds))
}

fn listening(port: u16) -> Option<String> {
    ["127.0.0.1", "[::1]"].into_iter().find_map(|host| {
        let addr: SocketAddr = format!("{host}:{port}").parse().ok()?;
        TcpStream::connect_timeout(&addr, Duration::from_millis(150)).ok()?;
        Some(format!("{host}:{port}"))
    })
}

fn target(arg: Option<String>) -> Result<Target> {
    let authority = match arg {
        Some(arg) => {
            let arg = arg.trim_start_matches("http://").trim_end_matches('/');
            match arg.parse::<u16>() {
                Ok(port) => listening(port).unwrap_or_else(|| format!("127.0.0.1:{port}")),
                Err(_) if arg.contains(':') => arg.replace("localhost", "127.0.0.1"),
                Err(_) => bail!("{arg} isn't a port. Try: hostline 8080"),
            }
        }
        None => {
            let open: Vec<String> = std::thread::scope(|s| {
                let probes: Vec<_> =
                    USUAL_PORTS.iter().map(|&p| s.spawn(move || listening(p))).collect();
                probes.into_iter().filter_map(|p| p.join().ok().flatten()).collect()
            });
            match open.as_slice() {
                [one] => one.clone(),
                [] => bail!("nothing is listening on the usual ports. Say which one: hostline 8080"),
                many => {
                    let ports: Vec<&str> =
                        many.iter().filter_map(|a| a.rsplit(':').next()).collect();
                    bail!(
                        "more than one port is listening ({}). Say which one: hostline {}",
                        ports.join(", "),
                        ports[0]
                    )
                }
            }
        }
    };
    Ok(Target { authority })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Visitor {
    offer: String,
    answer: Option<Arc<Message>>,
    pc: Option<Arc<dyn webrtc::peer_connection::PeerConnection>>,
}

async fn run() -> Result<()> {
    let args = args()?;
    let target = Arc::new(target(args.target)?);
    let name = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "localhost".into());

    let room = Arc::new(Room::new());
    let keys = Keys::generate();
    let host_id = keys.public_key().to_hex()[..signal::HOST_ID_LEN].to_string();
    let shell = std::env::var("HOSTLINE_SHELL").unwrap_or_else(|_| SHELL.into());
    let link = format!("{shell}#{}~{host_id}", room.secret);
    let client = signal::connect(&room).await?;

    let ends = args.duration.map(|d| now_ms() + d.as_millis() as u64);
    print_link(&link, &target.authority, args.duration);
    // An installed copy swaps itself for a newer release while it runs (src/update.rs).
    tokio::spawn(async {
        if let Some(tag) = update::run().await {
            let line = format!("Hostline {tag} is installed. The next run uses it.");
            println!("  {}", paint("2", &line));
        }
    });

    let (end_tx, end_rx) = watch::channel(false);
    let (note_tx, mut notes) = mpsc::unbounded_channel();
    let visitors: Arc<Mutex<HashMap<PublicKey, Visitor>>> = Default::default();
    // one cookie jar per browser (its `v`), kept across its reloads and tabs until the link ends
    let jars: Mutex<HashMap<String, Arc<Cookies>>> = Default::default();
    let mut events = client.notifications();
    let deadline = tokio::time::sleep(args.duration.unwrap_or(Duration::from_secs(u32::MAX as u64)));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(n) = events.next() => {
                let ClientNotification::Event { event, .. } = n else { continue };
                let Ok(Message::Offer { sdp, who, v }) = room.open(&event.content) else { continue };
                let from = event.pubkey;
                let repeat = {
                    let mut v = visitors.lock().unwrap();
                    match v.get(&from) {
                        // the visitor sends its offer again until it hears back
                        Some(seen) if seen.offer == sdp => Some(seen.answer.clone()),
                        _ => {
                            v.insert(from, Visitor { offer: sdp.clone(), answer: None, pc: None });
                            None
                        }
                    }
                };
                match repeat {
                    Some(Some(answer)) => {
                        let (client, keys, room) = (client.clone(), keys.clone(), room.clone());
                        tokio::spawn(async move { signal::send(&client, &keys, &room, &answer).await });
                    }
                    Some(None) => {} // still answering
                    None => {
                        let (client, keys, room) = (client.clone(), keys.clone(), room.clone());
                        let (target, notes, ending) = (target.clone(), note_tx.clone(), end_rx.clone());
                        let (visitors, name) = (visitors.clone(), name.clone());
                        let jar = if v.is_empty() {
                            Default::default()
                        } else {
                            jars.lock().unwrap().entry(v).or_default().clone()
                        };
                        tokio::spawn(async move {
                            match peer::answer(sdp, who, jar, target, notes, ending).await {
                                Ok((pc, sdp)) => {
                                    let answer = Arc::new(Message::Answer { to: from.to_hex(), sdp, name, ends });
                                    if let Some(v) = visitors.lock().unwrap().get_mut(&from) {
                                        v.answer = Some(answer.clone());
                                        v.pc = Some(pc);
                                    }
                                    let _ = signal::send(&client, &keys, &room, &answer).await;
                                }
                                Err(e) => eprintln!("  couldn't answer a visitor: {e:#}"),
                            }
                        });
                    }
                }
            }
            Some(note) = notes.recv() => match note {
                Note::Joined(who) => println!("  {} {who} opened the link", paint("32", "●")),
                Note::Left(who) => println!("  {}", paint("2", &format!("○ {who} left"))),
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = &mut deadline => {
                println!("\n  The {} are up. The link ended.", humane(args.duration.unwrap_or_default()));
                break;
            }
        }
    }

    // Tell every open page the link ended, then let go of everything.
    let _ = end_tx.send(true);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let pcs: Vec<_> = visitors.lock().unwrap().drain().filter_map(|(_, v)| v.pc).collect();
    for pc in pcs {
        let _ = pc.close().await;
    }
    client.shutdown().await;
    Ok(())
}

fn humane(d: Duration) -> String {
    let s = d.as_secs();
    let (n, unit) = match s {
        _ if s >= 3600 && s.is_multiple_of(3600) => (s / 3600, "hour"),
        _ if s >= 60 && s.is_multiple_of(60) => (s / 60, "minute"),
        _ => (s, "second"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// ANSI styling, only in a terminal and never with `NO_COLOR` set.
fn paint(style: &str, text: &str) -> String {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        format!("\x1b[{style}m{text}\x1b[0m")
    } else {
        text.into()
    }
}

fn print_link(link: &str, authority: &str, duration: Option<Duration>) {
    use std::io::IsTerminal;
    let copied = arboard::Clipboard::new().and_then(|mut c| c.set_text(link)).is_ok();
    println!();
    println!("  {}  sharing {authority}", paint("1", "hostline"));
    println!();
    println!("  {link}");
    let hint = if copied { "copied · scan it to open on your phone" } else { "scan it to open on your phone" };
    println!("  {}", paint("2", hint));
    println!();
    // a QR code only helps where someone can point a camera at it
    if std::io::stdout().is_terminal()
        && let Ok(code) = QrCode::new(link.as_bytes())
    {
        let art = code.render::<Dense1x2>().quiet_zone(true).build();
        for line in art.lines() {
            // black on white in any terminal theme, so every camera reads it
            println!("  {}", paint("30;47", line));
        }
        println!();
    }
    let ends = match duration {
        Some(d) => format!("The link ends in {}, or when you stop hostline (Ctrl+C).", humane(d)),
        None => "The link ends when you stop hostline (Ctrl+C).".into(),
    };
    println!("  {}", paint("2", &ends));
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_durations() {
        assert_eq!(duration("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(duration("10").unwrap(), Duration::from_secs(600));
        assert!(duration("1d").is_err());
        assert_eq!(humane(Duration::from_secs(3600)), "1 hour");
        assert_eq!(humane(Duration::from_secs(1800)), "30 minutes");
    }
}
