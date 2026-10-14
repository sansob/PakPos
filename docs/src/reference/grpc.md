# gRPC

Choose **gRPC** at the bottom of the method dropdown and enter the server URL:

| URL | Meaning |
|---|---|
| `grpc://localhost:50051` or `localhost:50051` | plaintext HTTP/2 |
| `grpcs://api.example.com` or `https://api.example.com` | TLS (system root certificates) |

![gRPC](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/grpc.png)

## Describe the service

PakPos needs the service's schema to build messages:

- **Server reflection** (default): click **Load methods**; PakPos asks the server
  (`grpc.reflection.v1`, falling back to `v1alpha`). Most frameworks can enable it
  with one line (tonic-reflection, grpc-go `reflection.Register`, grpcio-reflection…).
- **.proto files**: expand **Schema**, click **Add .proto…** and add import paths if
  your files import others. The files are compiled inside PakPos; no `protoc` needed.
  Well-known types (`google/protobuf/*.proto`) are built in.

Pick a method from the list; **Generate example** fills the message editor with
a JSON skeleton of the input type.

## Messages, metadata and auth

- **Message**: protobuf JSON mapping (field names in `lowerCamelCase` or as
  declared; enums as names; 64-bit integers as strings). `{{variables}}` are
  resolved first. For client-streaming and bidirectional methods, write a JSON
  **array**: every element is sent, then the responses are read.
- **Metadata** tab: request metadata (headers). **Auth** (Bearer, API key,
  OAuth 2.0…) is sent as `authorization` metadata like for HTTP.

## Responses

The response panel shows the gRPC status (`0 Ok`, `5 NotFound`, …), the message
as JSON (several messages as an array for server streaming, at most 500), and the
response headers and trailers (`grpc-status`, `grpc-message`, `trailer: …`).
[Tests](tests.md) work on gRPC responses: `status` is the gRPC code and
`json.<path>` reads the message.

```text
status          equals   0
json.status     equals   SERVING
```

## From the terminal

```sh
pakpos-cli grpc grpc://localhost:50051                               # list methods
pakpos-cli grpc grpc://localhost:50051 grpc.health.v1.Health/Check -d '{"service":""}'
pakpos-cli grpc grpcs://api.example.com shop.v1.Orders/Get -d @order.json \
  --proto protos/shop.proto -I protos -H 'authorization: Bearer {{token}}' -e staging
pakpos-cli grpc grpc://localhost:50051 shop.v1.Orders/Create --template
```

Exit code is `1` when the status is not OK.

## Try it offline

```sh
cargo run -p pakpos-core --example grpc_demo_server   # grpc://127.0.0.1:50051
```

It serves the standard health service and `pakpos.demo.Chat`, with reflection. The demo collection's
**Realtime → Health check (gRPC)** request calls it. `Health/Watch` on the
service `pakpos.demo` streams a status change every 2 seconds.

## Interactive streams

For client-streaming and bidirectional methods, **▶ Open stream** keeps the
call open. Each **Send message** sends the editor's JSON (an array sends each
element), and answers appear in the message log as they arrive, like a
WebSocket. **End stream** half-closes the call (the server sees the end of
your messages and finishes); **Cancel** aborts it. The final status and
trailers are logged. Invalid JSON for the message type is reported without
closing the stream.

**Send** still works for these methods too: it sends a JSON array of messages
at once and waits for the result, which is also how the runner calls them.

The demo server has `pakpos.demo.Chat/Talk` (bidirectional, answers every
message) and `pakpos.demo.Chat/Sum` (client streaming); the demo collection's
**Chat (gRPC stream)** request uses `Talk`.

![Bidirectional stream](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/grpc-stream.png)

## Limitations

- TLS uses the system's root certificates plus the CA of a matching client
  certificate entry ([mTLS](auth.md#client-certificates-mtls)); the *verify
  TLS* switch is not applied to gRPC.
- Binary metadata (`-bin` keys) is shown as a byte count and cannot be sent.

gRPC requests run in the collection runner: the status code is the `status`
target (`0` = OK) and the response message is the body.
