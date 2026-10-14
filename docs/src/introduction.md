# Introduction

![PakPos](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/overview.png)

**PakPos** is a fast, native, open-source API client written in Rust. It lets you
build HTTP requests, organise them in collections, switch between environments,
test responses, and run whole collections from the command line or CI.

It speaks HTTP, GraphQL, WebSocket and gRPC, and ships as two programs built on one engine:

| Program | What it is |
|---|---|
| `pakpos` | The desktop application (Windows, macOS, Linux). |
| `pakpos-cli` | A headless runner for terminals and CI, gRPC / WebSocket clients, a mock server, a collection and cURL converter, and an MCP server for AI agents. |

## Design principles

1. **Local first.** No account, no telemetry, no cloud. Everything lives in a folder on your disk.
2. **Plain files.** One JSON file per collection/environment, pretty-printed and stable, so diffs are readable.
3. **Safe by default.** Declarative tests need no code at all; JavaScript scripts run in a sandbox and only for collections you trust.
4. **Open formats.** Import existing Collection v2.1 files and cURL commands; export back to v2.1.

## Where to go next

- New user? Start with [Installation](installation.md) and [Your first request](tutorial/first-request.md).
- Have existing collections? See [Importing collections and cURL](tutorial/import.md).
- Want to contribute? Read [Architecture](architecture.md) and the [development guide](contributing/development.md).
