# Code signing policy

**Status:** PakPos is applying for free code signing through the
[SignPath Foundation](https://signpath.org/). Until the application is
approved, Windows builds are **not signed** (see
[Installation](installation.md) for what Windows shows the first time).
When signing starts, this line will say:
*Free code signing provided by [SignPath.io](https://about.signpath.io/),
certificate by [SignPath Foundation](https://signpath.org/).*

## What gets signed

Only the Windows release files that GitHub Actions builds from a tagged commit
on `main` in [github.com/sansob/PakPos](https://github.com/sansob/PakPos):

- `pakpos.exe` (the app) and `pakpos-cli.exe` (the command line), in the
  installer and the portable zip, for x64 and ARM64;
- the installer `PakPos-<version>-windows-<arch>-setup.exe`.

Nothing built on a developer's machine is ever signed, and no third-party
binary is shipped or signed: PakPos is built from source with Cargo, and every
dependency is open source. The bundled fonts are under the SIL Open Font
License 1.1 (`assets/fonts/`).

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [sansob](https://github.com/sansob) |
| Approvers | [sansob](https://github.com/sansob) |

Every change reaches `main` through a pull request with CI green on Linux,
Windows and macOS. Every signing request is approved by hand by an approver,
after checking that it comes from the release workflow of a tagged commit.
Team members use multi-factor authentication on GitHub and SignPath.

## Privacy policy

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or
operating it.

In practice, PakPos only connects to the network when you ask it to: when you
send a request or run a collection (to the URLs in those requests), log in with
OAuth 2.0, fetch a GraphQL schema or gRPC reflection, open a WebSocket, SSE,
Socket.IO, MQTT or MCP connection, start the mock server (locally), or pull and
push with Git sync (to the remote you configured). There is no telemetry, no
crash reporting and no automatic update check. Your collections, environments,
history, tokens and cookies stay in your workspace folder.

## Reporting

Report a signed file you believe is malicious, or any problem with this policy,
by opening an issue at
[github.com/sansob/PakPos/issues](https://github.com/sansob/PakPos/issues).
