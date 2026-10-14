# WebSocket

Choose **WebSocket** at the bottom of the method dropdown. The URL bar's button
becomes **Connect / Disconnect**.

![WebSocket](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/websocket.png)

- **URL**: `ws://` or `wss://` (`http(s)://` is converted). Query **Params**,
  **Headers** and **Auth** (including OAuth 2.0) are applied to the upgrade request.
- **Message** tab: compose a message (JSON or text, `{{variables}}` resolved on
  send) and press **Send message** or `Ctrl/⌘ + Enter`. The draft is saved with
  the request.
- **Messages** panel: sent (↑ orange) and received (↓ green) messages with
  timestamps, JSON pretty-printed, filter box, click a message to copy it. The
  log keeps the last 5,000 messages.

Collections can mix HTTP and WebSocket requests. The collection runner and
`pakpos-cli run` skip WebSocket requests.

## From the terminal

```sh
pakpos-cli ws wss://stream.example.com/prices -H 'Authorization: Bearer {{token}}' -e staging \
  -s '{"type":"subscribe","symbol":"IDR"}'
```

Received messages go to stdout (pipe them into `jq`), connection info and sent
messages to stderr. Lines typed on stdin are sent as messages. `--timeout 10`
exits after ten seconds.

## Limitations

- WebSocket connections always verify TLS certificates and don't use the HTTP
  proxy setting.
- Binary frames are shown as text when they are valid UTF-8, otherwise as a byte count.
- Socket.IO, STOMP and other protocols on top of WebSocket are not decoded.

## Try it offline

`python3 scripts/mock_httpbin.py` serves a WebSocket on `ws://127.0.0.1:8787/ws`
that echoes messages and pushes a price tick every 2 seconds. Use the demo
collection's **Realtime → Live prices (WebSocket)** request with the *Local mock* environment.
