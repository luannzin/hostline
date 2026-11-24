# Hostline

Your localhost as a link.

```
npx hostline 5173
```

Hostline prints a link and a QR code. Whoever opens the link gets that port in their browser,
loaded straight from your machine over WebRTC. They install nothing and make no account, and
there's no tunnel server carrying your traffic. The link ends when you stop Hostline.

The first run downloads the Hostline binary for your system from the matching
[GitHub release](https://github.com/luannzin/hostline/releases) and keeps it in your cache.

`hostline --for 30m` ends the link after 30 minutes. With no port, Hostline shares the one usual
port that's listening (3000, 5173, 8080 and more).

How it works, its limits and what the relays see: [github.com/luannzin/hostline](https://github.com/luannzin/hostline)
· [hostline.luannzin.com](https://hostline.luannzin.com)
