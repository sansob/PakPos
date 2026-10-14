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

It serves the standard health service with reflection. The demo collection's
**Realtime → Health check (gRPC)** request calls it. `Health/Watch` on the
service `pakpos.demo` streams a status change every 2 seconds.

## Limitations

- No interactive bidirectional streaming: inputs are sent up front.
- TLS uses the system's root certificates; client certificates (mTLS) and the
  *verify TLS* switch are not applied to gRPC yet.
- Binary metadata (`-bin` keys) is shown as a byte count and cannot be sent.
- gRPC requests are skipped by the collection runner.
