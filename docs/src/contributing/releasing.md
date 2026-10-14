# Releasing

Releases are built by `.github/workflows/release.yml` when a `vX.Y.Z` tag is pushed.

## Steps

1. Branch: `git checkout develop && git pull && git checkout -b release/0.2`
2. Bump `version` in the root `Cargo.toml` (`[workspace.package]` and the
   `pakpos-core` entry in `[workspace.dependencies]`), then run `cargo build`
   to refresh `Cargo.lock`.
3. Move the *Unreleased* notes in `CHANGELOG.md` under `## [0.2.0] - YYYY-MM-DD`.
4. Open a PR `release/0.2 → main`, wait for CI, merge.
5. Tag and push:

   ```sh
   git checkout main && git pull
   git tag -a v0.2.0 -m "PakPos 0.2.0"
   git push origin v0.2.0
   ```

6. Merge `main` back into `develop`.

The workflow checks that the tag matches the Cargo version, builds every platform,
and publishes a GitHub Release with the changelog section as notes. Tags with a
suffix (`v0.2.0-beta.1`) are marked as pre-releases.

## What gets built

| Job | Runner | Artifacts |
|---|---|---|
| linux x86_64 | `ubuntu-22.04` | `.tar.gz`, `.deb` |
| linux aarch64 | `ubuntu-22.04-arm` | `.tar.gz`, `.deb` |
| windows | `windows-latest` | portable `.zip`, Inno Setup `-setup.exe` |
| macos | `macos-latest` | universal (arm64 + x86_64) `.dmg`, CLI `.tar.gz` |

Plus `SHA256SUMS.txt`. Linux builds use Ubuntu 22.04 so the binaries run on
older glibc versions.

Re-run a release for an existing tag via **Actions → Release → Run workflow**.

## Code signing (future)

Unsigned builds trigger SmartScreen/Gatekeeper warnings. To sign later:

- **macOS:** add an Apple Developer ID certificate as secrets, replace the
  ad-hoc `codesign -s -` in `packaging/macos/make-dmg.sh`, then `notarytool submit` + `stapler staple`.
- **Windows:** sign `pakpos.exe` and the installer with `signtool` (or Azure Trusted Signing) in the windows job.
