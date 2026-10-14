# Contributing to PakPos

Thanks for wanting to help! Bug reports, documentation fixes, translations and
code are all welcome, and you don't need to be a Rust expert to start.

## Quick version

1. Look for an existing issue, or open one to talk about bigger changes first.
2. Fork, then branch from `develop`: `git checkout -b feature/my-thing develop`.
3. Make the change, with a test if it touches `pakpos-core`.
4. Run `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
5. Open a pull request **against `develop`** and fill in the template.

Good first issues are labelled
[`good first issue`](https://github.com/sansob/PakPos/labels/good%20first%20issue).

## More detail

- [Development guide](docs/src/contributing/development.md): setup, scripts, where things live.
- [Branching model](docs/src/contributing/branching.md): `main`, `develop`, `feature/*`, `release/*`, `hotfix/*`.
- [Releasing](docs/src/contributing/releasing.md): how a version gets shipped.
- [Architecture](docs/src/architecture.md): how the crates fit together.

## Reporting bugs

Please include your OS, PakPos version (status bar or `pakpos-cli --version`),
what you did, what you expected and what happened. If a request misbehaves,
`pakpos-cli send …` output or the **Request** tab of the response helps a lot.
Remove tokens and passwords before posting!

Security problems: see [SECURITY.md](SECURITY.md), not public issues.

## License

By contributing you agree that your work is dual-licensed under MIT and
Apache-2.0, like the rest of the project.

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
