//! Finding each other: the host and every visitor meet in a room on public Nostr relays.
//!
//! A link is `#<secret>~<host>`. The secret never reaches a relay: the room is a hash of it, and
//! every offer and answer is sealed with a key derived from it, so the relays only ever carry
//! ciphertext under a tag nobody can turn back into the link. The events are ephemeral (kind
//! 20000 to 29999), which relays pass on and never store. `<host>` is the start of the host's
//! Nostr key, so someone else holding the link can't answer in the host's place.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nostr_sdk::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Ephemeral, so relays forward it and keep nothing.
pub const KIND: u16 = 21983;
/// The shell's own copy is in the lines repo (`src/app/hostline/shell/signal.ts`).
pub const RELAYS: [&str; 5] = [
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.primal.net",
    "wss://relay.nostr.net",
    "wss://relay.snort.social",
];
/// How many hex characters of the host's key the link carries (88 bits).
pub const HOST_ID_LEN: usize = 22;

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Message {
    /// A visitor's offer; `who` is how the visitor's browser names itself ("Safari on iPhone").
    Offer { sdp: String, who: String },
    /// The host's answer to one visitor, addressed by that visitor's Nostr key.
    Answer {
        to: String,
        sdp: String,
        name: String,
        /// When the link ends, in Unix milliseconds, if the host set `--for`.
        #[serde(skip_serializing_if = "Option::is_none")]
        ends: Option<u64>,
    },
}

pub struct Room {
    pub secret: String,
    pub tag: String,
    key: Aes256Gcm,
}

impl Room {
    pub fn new() -> Self {
        // 22 Base62 characters, 131 bits, like Sightline's links
        let mut rng = rand::rng();
        let secret: String = (0..22).map(|_| BASE62[rng.random_range(0..62)] as char).collect();
        Self::from_secret(secret)
    }

    pub fn from_secret(secret: String) -> Self {
        let hash = Sha256::digest(format!("hostline:{secret}").as_bytes());
        let tag = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
        let mut okm = [0u8; 32];
        hkdf::Hkdf::<Sha256>::new(Some(b"hostline"), secret.as_bytes())
            .expand(b"signal", &mut okm)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        let key = Aes256Gcm::new_from_slice(&okm).expect("32-byte key");
        Self { secret, tag, key }
    }

    pub fn seal(&self, message: &Message) -> Result<String> {
        let mut iv = [0u8; 12];
        rand::rng().fill(&mut iv);
        let plain = serde_json::to_vec(message)?;
        let sealed = self
            .key
            .encrypt(&Nonce::from(iv), plain.as_slice())
            .map_err(|_| anyhow!("encrypt"))?;
        Ok(B64.encode([iv.as_slice(), &sealed].concat()))
    }

    pub fn open(&self, content: &str) -> Result<Message> {
        let bytes = B64.decode(content)?;
        let (iv, sealed) = bytes.split_at_checked(12).context("short message")?;
        let plain = self
            .key
            .decrypt(&Nonce::try_from(iv)?, sealed)
            .map_err(|_| anyhow!("not sealed with this link"))?;
        Ok(serde_json::from_slice(&plain)?)
    }
}

pub async fn connect(room: &Room) -> Result<Client> {
    let client = Client::new();
    for relay in RELAYS {
        client.add_relay(relay).await?;
    }
    client.connect().await;
    client
        .subscribe(
            Filter::new()
                .kind(Kind::Custom(KIND))
                .custom_tag(SingleLetterTag::LOWERCASE_X, room.tag.clone())
                .since(Timestamp::now() - 10),
        )
        .await?;
    Ok(client)
}

pub async fn send(client: &Client, keys: &Keys, room: &Room, message: &Message) -> Result<()> {
    let event = EventBuilder::new(Kind::Custom(KIND), room.seal(message)?)
        .tag(Tag::parse(["x", room.tag.as_str()])?)
        .finalize(keys)?;
    client.send_event(&event).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let room = Room::from_secret("0123456789abcdefABCDEF".into());
        let sealed = room
            .seal(&Message::Offer { sdp: "v=0".into(), who: "test".into() })
            .unwrap();
        assert!(matches!(room.open(&sealed).unwrap(), Message::Offer { sdp, .. } if sdp == "v=0"));
        assert!(Room::from_secret("x".repeat(22)).open(&sealed).is_err());
    }

    // The page's `signal.test.ts` checks it gets the same room for the same secret.
    #[test]
    fn matches_the_page() {
        let room = Room::from_secret("0123456789abcdefABCDEF".into());
        assert_eq!(room.tag, "239d7edac96ad773488af98bf4b8e368");
    }
}
