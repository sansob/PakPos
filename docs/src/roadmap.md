# Roadmap

Ideas, roughly by priority. Open an issue before starting something big.

**Done in 0.2:** OAuth 2.0, GraphQL with schema explorer, WebSocket, gRPC,
sandboxed JavaScript scripts, examples + mock server, Git sync.

**Next**
- Digest, AWS Signature v4 and NTLM auth; client certificates (mTLS), also for gRPC
- Data-driven runs (CSV / JSON iterations) and running gRPC requests in the runner
- Cookie jar viewer/editor
- Response search, JSON tree view, image/HTML preview
- Drag & drop to reorder requests and folders
- `pm.sendRequest` behind an explicit per-collection network permission

**Later**
- Interactive bidirectional gRPC streaming
- Socket.IO, Server-Sent Events, MQTT
- MCP *client* request type (test MCP servers, like a REST request)
- OpenAPI 3 import / export
- Code-signed and notarised builds; Homebrew, winget, Flatpak, AUR packages
- Translations of the UI (Bahasa Indonesia first)

**Probably not**
- Hosted cloud accounts and sync: Git sync covers sharing without a server.
