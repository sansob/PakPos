# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/sansob/PakPos/compare/v0.2.0-rc.1...HEAD
[0.2.0-rc.1]: https://github.com/sansob/PakPos/releases/tag/v0.2.0-rc.1
