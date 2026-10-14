<p align="center">
  <img src="assets/icon-256.png" width="96" alt="PakPos logo">
</p>

<h1 align="center">PakPos</h1>

<p align="center">
  An API client that opens instantly, keeps your data on your machine,<br>
  and doesn't ask you to sign in. Written in Rust.
</p>

<p align="center">
  <a href="https://github.com/sansob/PakPos/actions/workflows/ci.yml"><img src="https://github.com/sansob/PakPos/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/sansob/PakPos/releases"><img src="https://img.shields.io/github/v/release/sansob/PakPos?include_prereleases" alt="Release"></a>
  <img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue" alt="License">
</p>

<p align="center">
  <b>English</b> · <a href="README.id.md">Bahasa Indonesia</a> · <a href="https://sansob.github.io/PakPos/">Docs</a>
</p>

![PakPos in dark mode](docs/screenshots/overview.png)

## Why this exists

Most of us just want to fire off a request, look at the JSON, tweak a header and
try again. Somewhere along the way API clients turned into heavy Electron apps
that want an account, sync your collections to someone else's cloud and take a
few seconds to start.

PakPos goes back to the basics and tries to do them well. It's a native app
(a single ~20 MB binary, no Electron), it starts before you can blink, and everything
you make is a plain JSON file in a folder you choose. That last part matters
more than it sounds: put the folder in your project's git repo and your whole
team gets the same collections, with changes reviewed in pull requests like the
rest of your code.

The name comes from Indonesian: *Pak Pos* is the postman who takes your letter
out and brings the reply back. That's the whole job.

## What it can do today

You can send any common HTTP method with query params, headers, and JSON, form,
multipart or binary bodies. Basic, Bearer and API-key auth are built in, and a
request can inherit auth from its folder or collection, so you set your token
once instead of forty times.

Requests live in collections with folders. Environments hold variables like
`{{base_url}}` and `{{token}}`: switch from Staging to Production in the header
and every request follows. Variables light up green when they resolve and red
when they don't, which saves a surprising amount of head-scratching.

Instead of JavaScript test scripts, PakPos uses small declarative checks such as
`status equals 200`, `json.data.id exists` or `time less than 500`. You can also
pull a value out of a response (say `json.token`) into a variable and use it in
the next request, which is all most login flows need. Because tests are data,
not code, running a teammate's collection never runs their code on your machine.

Already on Postman? Export your collection (v2.1) and environments and import
them here; most things come across as-is. You can also paste a `curl` command
straight into the URL bar, and go the other way with the `</>` button to get
cURL, Python, JavaScript or HTTPie snippets.

There's a light theme too, if that's your thing:

![PakPos in light mode](docs/screenshots/light.png)

## The command line, and CI

The same engine ships as `pakpos-cli`, so the collection you click through in
the app can run in your pipeline:

```sh
pakpos-cli run api/collection.json -e api/staging.json --var token=$API_TOKEN \
  --reporter junit --out report.xml
```

It exits non-zero when a check fails, speaks JUnit for GitHub Actions, GitLab
and Jenkins, and happily reads Postman files too.

## Letting an AI agent use your requests

`pakpos-cli mcp` runs a [Model Context Protocol](https://modelcontextprotocol.io)
server, so Claude, Cursor, VS Code and friends can list your collections, run a
saved request or a whole collection, and read the results. Be careful with this:
an agent that can send HTTP requests can reach anything your laptop can, so start
it with `--allow-host` and name only the APIs it should touch. The
[MCP tutorial](docs/src/tutorial/mcp.md) walks through it.

## Getting it

Grab a build for your OS from the [releases page](https://github.com/sansob/PakPos/releases):
an installer or portable zip for Windows, a `.dmg` for macOS (Apple Silicon and
Intel in one), and a `.deb` or tarball for Linux.

The builds aren't code-signed yet, so your OS will grumble the first time. On
macOS right-click the app and choose **Open**; on Windows click **More info →
Run anyway**. The [installation guide](docs/src/installation.md) has the details.

If you'd rather build it yourself, you need Rust 1.95 or newer:

```sh
git clone https://github.com/sansob/PakPos && cd PakPos
cargo run --release -p pakpos
```

The first launch creates a small demo collection, so you have something to press
**Send** on straight away (`Ctrl/⌘ + Enter` works too).

## What it doesn't do (yet)

It's a young project, so here's what's missing compared with Postman. There are
no JavaScript pre-request or test scripts (imported ones are skipped with a
warning), no OAuth 2.0 flows, and no GraphQL, WebSocket or gRPC request types.
There are no mock servers either, and no cloud sync, which is deliberate: use git
for that. The [roadmap](docs/src/roadmap.md) lists what's planned, and any of it
is up for grabs.

## Docs

The [documentation](https://sansob.github.io/PakPos/) (source in `docs/`) starts
with six short tutorials: your first request, environments and variables, tests
and chaining, moving from Postman, running in CI, and the MCP server. Then come
the references for the test language, CLI and file format.

## Helping out

Issues, ideas, doc fixes and translations are as welcome as code. Have a look at
[CONTRIBUTING.md](CONTRIBUTING.md); in short, branch from `develop`, keep
`cargo fmt`, `clippy` and the tests happy, and open a PR back into `develop`.
The [architecture notes](docs/src/architecture.md) explain how the three crates
(`pakpos-core`, `pakpos-cli` and the `pakpos` app) fit together.

## License

MIT or Apache-2.0, whichever suits you.

PakPos is an independent project and isn't affiliated with Postman, Inc.
