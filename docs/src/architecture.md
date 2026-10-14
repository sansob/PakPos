# Architecture

PakPos is a Cargo workspace with three crates. The engine has no UI code, so the
desktop app, the CLI and the MCP server behave identically.

```text
            ┌──────────────────────┐     ┌───────────────────────────────┐
            │  pakpos (desktop)    │     │  pakpos-cli                   │
            │  egui / eframe       │     │  clap · run/send/import/mcp   │
            └──────────┬───────────┘     └───────────────┬───────────────┘
                       │                                 │
                       ▼                                 ▼
            ┌──────────────────────────────────────────────────────────┐
            │ pakpos-core                                              │
            │  model ─ vars ─ http ─ assertions ─ runner ─ scripts     │
            │  oauth2 ─ graphql ─ ws ─ grpc ─ mock ─ git               │
            │  postman ─ codegen (cURL) ─ storage ─ samples            │
            └──────────────────────────────────────────────────────────┘
                       │ reqwest (blocking, rustls)   │ JSON files
                       ▼                              ▼
                    network                     workspace folder
```

## pakpos-core

| Module | Responsibility |
|---|---|
| `model` | Plain serde data: `Collection`, `Folder`, `Request`, `Body`, `Auth`, `Environment`, … |
| `vars` | `VarScope`: layered `{{variable}}` resolution and dynamic variables |
| `http` | `prepare` (resolve vars, params, auth, body) → `PreparedRequest`; `send` with a blocking `reqwest` client; host allowlist |
| `assertions` | Declarative tests, JSON paths, extractions |
| `runner` | Sequential collection runs with progress callback; JUnit output |
| `postman` | Postman Collection v2.0/2.1 and environment import/export |
| `codegen` | cURL parser and snippet generators |
| `storage` | Workspace on disk; atomic writes; tolerant loading |
| `oauth2` | Token requests, PKCE + local redirect listener, token store, `ensure_token` |
| `graphql` | Introspection query, schema parsing, starter queries |
| `ws` | WebSocket client on a worker thread, driven by channels |
| `grpc` | Dynamic gRPC (tonic + prost-reflect): reflection / `.proto` (protox), all call kinds |
| `scripts` | QuickJS sandbox, `pm.*` prelude, script chains |
| `mock` | Std-only HTTP mock server serving saved examples |
| `git` | Workspace-scoped git operations through the `git` executable |

`prepare` is separate from `send` so the UI can show exactly what will be sent,
code snippets are generated from the same data, and the MCP server can check the
final host before any network traffic.

## pakpos (desktop)

Built with [egui](https://github.com/emilk/egui) through `eframe`, using the
`glow` (OpenGL) renderer. egui is *immediate mode*: the whole UI is a function
of the state, redrawn on input, which keeps the code simple and the binary small.

- `app.rs`: `PakPosApp` state (tabs, dialogs, toasts, runner), frame loop, persistence.
- `sidebar.rs`, `request_view.rs`, `response_view.rs`, `dialogs.rs`: UI regions.
- `actions.rs`: UI code pushes `Action`s; they are applied after drawing, which avoids borrow conflicts.
- `theme.rs`: the navy/orange visual identity for dark and light mode.
- `oauth_view.rs`, `graphql_view.rs`, `ws_view.rs`, `grpc_view.rs`, `scripts_view.rs`,
  `mock_view.rs`, `git_view.rs`: one module per feature, each with its editor
  widgets and the background job it drives.
- `scene.rs`: scripted scenes used to generate the documentation screenshots.

Requests run on a background thread and report back over a channel, so the UI
never blocks. Open tabs are restored using eframe's storage; collections and
environments always go to the workspace folder.

**Why egui and not native OS widgets?** There is no mature cross-platform Rust
toolkit that wraps Win32, Cocoa and GTK widgets at the same time. egui renders its
own widgets on the GPU, which gives identical behaviour on all three OSes in one
codebase. The trade-off: it does not look like a stock macOS/Windows app, and
some accessibility/IME features lag behind native toolkits.

## pakpos-cli

`clap` front end over `pakpos-core`, plus `mcp.rs`, a small dependency-free MCP
server (JSON-RPC over stdio).

## Tests

- Unit tests next to the code in every core module.
- `crates/pakpos-core/tests/live_http.rs`: end-to-end tests against an in-process HTTP server.
- MCP protocol tests in `crates/pakpos-cli/src/mcp.rs`.
- CI also runs the example collection against `scripts/mock_httpbin.py`.
