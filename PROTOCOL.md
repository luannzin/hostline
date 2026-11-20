# Protocol

Two ends: the CLI (`src/`, Rust) and the page (`luannzin/lines`, `src/app/hostline/shell/` and
`public/hostline-sw.js`). A change to either side changes both.

## The link

`https://hostline.luannzin.com/<path>#<secret>~<host>`

- `secret`: 22 Base62 characters, 131 random bits, made fresh on every run.
- `host`: the first 22 hex characters of the CLI's Nostr public key (88 bits), made fresh on
  every run.
- `path`: the shared app's path. The page opens it in the frame.

## Meeting

Both ends join the same room on the same relays (`src/signal.rs`, `signal.ts`).

- Relays: `wss://relay.damus.io`, `wss://nos.lol`, `wss://relay.primal.net`,
  `wss://relay.nostr.net`, `wss://relay.snort.social`.
- Event kind `21983` (ephemeral), one tag `["x", <room>]`.
- `room`: the first 16 bytes of `SHA-256("hostline:" + secret)`, as 32 hex characters.
- `content`: Base64 of a 12-byte IV followed by AES-256-GCM ciphertext. The key is
  `HKDF-SHA256(ikm = secret, salt = "hostline", info = "signal")`, 32 bytes.

Plaintext messages, JSON:

| Message | From | Fields |
|---|---|---|
| `{"t":"offer"}` | page | `sdp`: the full offer, ICE gathered; `who`: "Safari on iPhone" |
| `{"t":"answer"}` | CLI | `to`: the page's Nostr key (hex); `sdp`: the full answer; `name`: the folder the CLI runs in; `ends`: Unix ms, only with `--for` |

The page signs with a key made per page load and sends its offer every 4 seconds until an answer
addressed to its key arrives from a key starting with `host`. The CLI answers each new offer once
and repeats the same answer when the same offer comes again.

## The line

One reliable, ordered data channel, label `hostline`, opened by the page. Every message is a frame:

```
byte 0     kind
bytes 1-4  id, big-endian u32 (the request or socket it belongs to)
bytes 5-   payload
```

Ids come from the page, one counter for requests and sockets together. Bodies travel in pieces of
at most 16 KiB.

| Kind | Name | Direction | Payload |
|---|---|---|---|
| 1 | `REQ` | page → CLI | JSON `{"m": method, "u": path and query, "h": [[name, value], ...]}` |
| 2 | `REQ_BODY` | page → CLI | bytes |
| 3 | `REQ_END` | page → CLI | none; the CLI sends the request now |
| 4 | `CANCEL` | page → CLI | none |
| 5 | `RES` | CLI → page | JSON `{"s": status, "t": status text, "h": [[name, value], ...]}` |
| 6 | `RES_BODY` | CLI → page | bytes |
| 7 | `RES_END` | CLI → page | none |
| 8 | `WS_OPEN` | page → CLI | JSON `{"u": path and query, "p": [subprotocols]}` |
| 9 | `WS_OPENED` | CLI → page | JSON `{"p": the chosen subprotocol}` |
| 10 | `WS_TEXT` | both | UTF-8 text, one WebSocket message |
| 11 | `WS_BIN` | both | bytes, one WebSocket message |
| 12 | `WS_CLOSE` | both | JSON `{"c": code, "r": reason}` |
| 13 | `FAIL` | CLI → page | text: why the request couldn't be made |
| 14 | `BYE` | CLI → page | none, id 0: the link ended |

WebSockets always go to the shared port, whatever host the page asked for.
