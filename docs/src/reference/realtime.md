# SSE, Socket.IO and MQTT

Besides [WebSocket](websocket.md), the method picker has three more realtime
kinds. They all work the same way: **Connect** opens a session, messages
appear in the log below (with their event name or topic as a tag, and a
filter), and **Disconnect** ends it. They are not run by the collection
runner because they are interactive.

## Server-Sent Events (SSE)

Opens the URL with `Accept: text/event-stream`, using the request's params,
headers and auth. Every event is shown with its type (`message` when the
server sends none); multi-line `data:` fields are joined, comments and
keep-alives are skipped. SSE is one-way, so there is no composer. If the
response is not a 2xx `text/event-stream`, the status and the start of the
body are shown instead.

![SSE](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/sse.png)

## Socket.IO

For servers using Socket.IO 3 or later (protocol v5 over Engine.IO v4, WebSocket transport).

| Field | Meaning |
|---|---|
| URL | `http://localhost:3000`; a path selects the namespace, e.g. `http://localhost:3000/admin` |
| Event | The event **Send message** emits; the editor's JSON (or text) is its payload |
| Auth | Optional JSON sent with the namespace connect, like `io(url, { auth })` |
| Path | Server path, default `/socket.io/` |

Received events are tagged with their name; several arguments are shown as
an array. Engine.IO pings are answered automatically. Binary attachments and
acknowledgement callbacks are not supported (acks the server sends are shown).

![Socket.IO](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/socketio.png)

## MQTT

MQTT 3.1.1 over TCP (`mqtt://host:1883`) or TLS (`mqtts://host:8883`).

- **Subscriptions**: topic filters (`+` and `#` wildcards) with a QoS. Enabled
  ones are subscribed on connect; while connected, *Subscribe* / *Unsubscribe*
  change them on the fly.
- **Publish**: topic, QoS 0 / 1 / 2 and *Retain*; **Send message** publishes
  the editor's content.
- **Connection**: client ID (random when empty), keep-alive and clean session.
- User name and password come from the **Auth** tab (Basic Auth).
- TLS uses the system's roots; a matching entry in **Settings → Client
  certificates** adds a private CA and a client certificate (mTLS). The
  *verify TLS* switch applies.

Messages are tagged with their topic, retained ones with `(retained)`.

![MQTT](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/mqtt.png)

## Try them offline

```sh
python3 scripts/mock_httpbin.py      # /sse and a Socket.IO server at /socket.io/
python3 scripts/mock_mqtt.py         # an MQTT broker on mqtt://127.0.0.1:1883
```

The demo collection's **Realtime** folder has a request for each.

## From the terminal

```sh
pakpos-cli sse http://127.0.0.1:8787/sse
pakpos-cli socketio http://127.0.0.1:8787 --emit 'track={"resi":"PP-2026-0042"}'
pakpos-cli mqtt mqtt://127.0.0.1:1883 --sub 'kantor/#:1'
pakpos-cli mqtt mqtts://broker.example --pub kantor/1/lampu -m '{"on":true}' --qos 1 \
  -u budi --password '{{mqtt_password}}' --cert client.pem --key client.key
```

Received messages go to stdout (`[event or topic] payload`), the rest to
stderr. `--timeout S` and `-c, --count N` end the session; stdin lines are
emitted (`event=payload` for Socket.IO) or published (MQTT, with `--pub`).
