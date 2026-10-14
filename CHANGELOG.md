# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- OAuth 2.0 auth: client credentials, password and authorization code with
  PKCE (local browser redirect). Tokens are cached in `tokens.json`, refreshed
  automatically, and used by the app, `pakpos-cli run` and the MCP server.
  Postman `oauth2` auth is imported and exported.

## [0.1.0-rc.1] - 2026-10-14

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
- Postman Collection v2.0/v2.1 and environment import/export; cURL import.
- Code snippets: cURL, Python `requests`, JavaScript `fetch`, HTTPie.
- `pakpos-cli`: `run` (CLI/JSON/JUnit reports), `send`, `import`, `export`, `curl`, `workspace`.
- `pakpos-cli mcp`: Model Context Protocol server with host allowlist.
- CI on all three OSes, release pipeline producing `.exe` installers and `.zip`
  for Windows x64 and ARM64, a universal `.dmg` for macOS, and `.deb` / `.tar.gz`
  for Linux x86_64 and arm64, plus a documentation site.

[Unreleased]: https://github.com/sansob/PakPos/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/sansob/PakPos/releases/tag/v0.1.0-rc.1
