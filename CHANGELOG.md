# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0-rc.1] - 2026-10-14

### Added
- The UI in Bahasa Indonesia (Settings → Language; follows the system locale
  by default), with a test that fails on untranslated UI text.
- Digest (RFC 7616: MD5, SHA-256, -sess, qop auth/auth-int), NTLMv2 and
  AWS Signature v4 auth, in the app, runner, CLI, MCP server, code snippets
  and Collection v2.1 import/export.
- Client certificates (mTLS) per host, with an optional private CA, for HTTPS
  and gRPC; `--cert`, `--key` and `--cacert` on `run`, `send` and `grpc`.
- Data-driven runs: CSV or JSON data files, one iteration per row, with
  `{{column}}` variables, `pm.iterationData` and `pm.info.iteration`;
  `pakpos-cli run --data / --iterations`, a run options dialog in the app and a
  `data` argument for the MCP `run_collection` tool.
- gRPC requests run in the collection runner (app, CLI and MCP).
- Persistent cookie jar (`cookies.json`, git-ignored) with RFC 6265 matching,
  cookies from redirects included, and a viewer/editor (**Cookies** in the top
  bar). Runs share the jar between requests.
- Response viewer: search with highlighted matches (`Ctrl/⌘ + F`), a JSON tree
  view (copy a value or its test path), image preview and a simplified HTML
  preview with *Open in browser*; *Save* picks the extension from the content type.
- OpenAPI 3.0 / 3.1 and Swagger 2.0 import (JSON or YAML): folders by tag,
  `{{baseUrl}}`, path variables, example bodies, auth from security schemes and
  response examples ready for the mock server; export to OpenAPI 3.0.3 YAML or
  JSON (app and `pakpos-cli export -f openapi|openapi-yaml`).
- Package manager manifests on every release (Homebrew cask and formula,
  winget, AUR `pakpos-bin`, Flatpak with metainfo), generated from the release
  checksums; release notes no longer include the changelog's link lines.
- MCP client request type: test MCP servers over Streamable HTTP or stdio
  like REST requests (call tools, read resources, get prompts, list tools),
  with tests, the runner and `pakpos-cli mcp-client`; stdio servers need a
  trusted collection. The mock API gains a small `/mcp` server.
- Server-Sent Events, Socket.IO (v5 / Engine.IO v4) and MQTT 3.1.1 (QoS 0-2,
  retained messages, TLS and client certificates) as request kinds with the
  live message log; `pakpos-cli sse`, `socketio` and `mqtt`; an offline
  Socket.IO server in the mock API and `scripts/mock_mqtt.py`.
- Interactive gRPC streams: open a client-streaming or bidirectional call,
  send messages one by one, see answers live, end or cancel it. The demo
  server gains `pakpos.demo.Chat` (`Talk`, `Sum`).
- `pm.sendRequest` in scripts, behind a per-collection network permission
  (trusted collections only, stored in settings); `pakpos-cli run --allow-network`;
  host allow-list, request cap and cookie jar apply.
- Drag & drop in the sidebar: reorder requests and folders, move them into
  folders or to another collection; open tabs follow moved requests.

### Changed
- Dark theme: neutral near-black surfaces instead of navy (the light theme keeps
  its navy header).

### Fixed
- Windows installer: "Add pakpos-cli to PATH" failed for all-users installs
  (`RegCreateKeyEx failed; code 87` on `HKLM\Environment`). It now writes the
  system environment key, and uninstall removes the folder from PATH again.
  A CI job installs and uninstalls the installer to check this.
- Windows: no more console windows flashing up when PakPos runs git (the
  Sync badge refreshes every minute) or starts a stdio MCP server.

## [0.2.0-rc.1] - 2026-10-14

Release candidate for the first public release.

### Added
- Desktop app (`pakpos`) for Windows, macOS and Linux with dark and light themes.
- Requests with all common methods, query params synced with the URL, headers,
  raw / urlencoded / multipart / binary bodies, Basic / Bearer / API-key auth
  with inheritance from folders and collections.
- Collections with nested folders, environments, collection variables, globals
  and dynamic variables; live `{{variable}}` highlighting.
- Declarative tests and extractions; collection runner.
- Response viewer with syntax highlighting, headers, test results and the sent request.
- History, tabs restored between sessions, keyboard shortcuts.
- Collection v2.0/v2.1 and environment import/export; cURL import.
- Code snippets: cURL, Python `requests`, JavaScript `fetch`, HTTPie.
- `pakpos-cli`: `run` (CLI/JSON/JUnit reports), `send`, `import`, `export`, `curl`, `workspace`.
- `pakpos-cli mcp`: Model Context Protocol server with host allowlist.
- CI on all three OSes, release pipeline producing `.exe` installers and `.zip`
  for Windows x64 and ARM64, a universal `.dmg` for macOS, and `.deb` / `.tar.gz`
  for Linux x86_64 and arm64, plus a documentation site.
- OAuth 2.0 auth: client credentials, password and authorization code with
  PKCE (local browser redirect). Tokens are cached in `tokens.json`, refreshed
  automatically, and used by the app, `pakpos-cli run` and the MCP server.
  OAuth 2.0 auth in Collection v2.1 files is imported and exported.
- GraphQL body type with query, JSON variables and operation name, a schema
  explorer powered by introspection, and GraphQL bodies in Collection v2.1 import/export.
- WebSocket requests: connect with params, headers and auth, compose and send
  messages, live message log; `pakpos-cli ws` for the terminal.
- JavaScript pre-request and post-response scripts with a `pm.*` API (`pm.test`,
  chai-style `expect`, console) at collection, folder and request level, run in
  a QuickJS sandbox without file/network access and with time/memory limits.
  Imported collections must be trusted before their scripts run; the CLI needs
  `--allow-scripts`. Scripts in imported collections are kept instead of dropped.
- Collection settings dialog: description, collection variables and scripts.
- Saved examples and a mock server that serves them (`pakpos-cli mock`, or
  "Start mock server" in the app) with path parameters, example selection via
  `x-mock-response-code` / `x-mock-response-name`, CORS and a request log.
  Saved responses in Collection v2.1 files are imported and exported.
- gRPC requests with dynamic messages: service descriptions from server
  reflection or `.proto` files (compiled in-process), unary / server / client /
  bidirectional streaming, metadata and auth, JSON templates, status and
  trailers in the response, tests on gRPC responses; `pakpos-cli grpc`; a demo
  server example.
- Built-in Git sync instead of a cloud: initialize, set a remote, commit, pull
  (rebase, auto-reload), push, or one-click Sync, scoped to the workspace folder,
  using the installed git and its credentials; personal files are git-ignored.

[Unreleased]: https://github.com/sansob/PakPos/compare/v0.3.0-rc.1...HEAD
[0.3.0-rc.1]: https://github.com/sansob/PakPos/compare/v0.2.0-rc.1...v0.3.0-rc.1
[0.2.0-rc.1]: https://github.com/sansob/PakPos/releases/tag/v0.2.0-rc.1
