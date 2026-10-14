# Roadmap

Ideas, roughly by priority. Open an issue before starting something big.

**Done in 0.2:** OAuth 2.0, GraphQL with schema explorer, WebSocket, gRPC,
sandboxed JavaScript scripts, examples + mock server, Git sync.

**Done since 0.2 (next release):** Digest, AWS Signature v4 and NTLM auth;
client certificates (mTLS) for HTTPS and gRPC; data-driven runs (CSV / JSON)
and gRPC in the runner; cookie jar viewer/editor; response search, JSON tree,
image and HTML preview; drag & drop in the sidebar; `pm.sendRequest` behind a
per-collection network permission; interactive gRPC streams; Server-Sent
Events, Socket.IO and MQTT; MCP client requests; OpenAPI 3 / Swagger 2 import
and OpenAPI 3 export; package manager manifests (Homebrew, winget, Flatpak,
AUR); the UI in Bahasa Indonesia.

**Next**
- Publish the package manifests: a Homebrew tap, winget-pkgs, Flathub and the
  AUR (each needs a maintainer account; see [Package managers](contributing/packaging.md))
- More UI languages, and translating the CLI's messages
- npm-style libraries in scripts (a vetted, bundled set rather than `require`)
- Socket.IO and MQTT in the collection runner and the CLI test reporters
- MQTT 5

**Later**
- Code-signed and notarised builds (needs paid Apple and Windows certificates;
  not planned while PakPos is used by developers who can allow an unsigned app)
- HTTP/3

**Probably not**
- Hosted cloud accounts and sync: Git sync covers sharing without a server.
