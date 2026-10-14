# Development guide

## Setup

```sh
git clone https://github.com/sansob/PakPos && cd PakPos
git checkout develop
cargo run -p pakpos            # desktop app (debug)
cargo run -p pakpos-cli -- -h  # CLI
```

Rust 1.95+ (the workspace's `rust-toolchain.toml` selects stable and installs
`rustfmt` + `clippy`). Linux also needs:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Use a throw-away workspace while developing so you don't touch your real data:

```sh
PAKPOS_HOME=/tmp/pakpos-dev cargo run -p pakpos
```

Deleting that folder brings back the first-run demo collection.

## Before you push

These are exactly what CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Handy scripts

| Script | Purpose |
|---|---|
| `python3 scripts/mock_httpbin.py [port]` | Offline httpbin-compatible API on `127.0.0.1:8787` |
| `./scripts/screenshots.sh [scene…]` | Regenerate `docs/screenshots/*.png` (needs `xvfb-run`) |
| `python3 scripts/make_icons.py` | Regenerate the icons in `assets/` (needs Pillow) |
| `cargo run -p pakpos-core --example export_samples -- examples` | Regenerate `examples/` |

## Documentation

The docs are an [mdBook](https://rust-lang.github.io/mdBook/):

```sh
cargo install mdbook
mdbook serve docs --open
```

They are published to GitHub Pages from `main` by `.github/workflows/docs.yml`.

## Where to change things

| You want to… | Look at |
|---|---|
| add a request/body/auth option | `pakpos-core/src/model.rs`, then `http.rs::prepare`, `postman.rs`, the editor in `pakpos-app/src/request_view.rs` |
| add a test operator or target | `pakpos-core/src/assertions.rs` |
| support another cURL flag | `pakpos-core/src/codegen.rs::parse_curl` |
| add a CLI command | `pakpos-cli/src/main.rs` |
| add an MCP tool | `pakpos-cli/src/mcp.rs` (`tools()` and `call()`) |
| change colours / spacing | `pakpos-app/src/theme.rs` |

Please add a unit test with every behaviour change in `pakpos-core`.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org): `feat(app): …`,
`fix(core): …`, `docs: …`, `ci: …`, `refactor: …`, `test: …`, `chore: …`.
They make the changelog easy to write.
