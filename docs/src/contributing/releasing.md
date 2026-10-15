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
| windows x64 | `windows-latest` | portable `.zip`, Inno Setup `-setup.exe` |
| windows arm64 | `windows-11-arm` | portable `.zip`, Inno Setup `-setup.exe` |
| macos | `macos-latest` | universal (arm64 + x86_64) `.dmg`, CLI `.tar.gz` |

Plus `SHA256SUMS.txt`. Linux builds use Ubuntu 22.04 so the binaries run on
older glibc versions.

Re-run a release for an existing tag via **Actions → Release → Run workflow**.

## Code signing (future)

Unsigned builds trigger SmartScreen/Gatekeeper warnings. To sign later:

- **macOS:** add an Apple Developer ID certificate as secrets, replace the
  ad-hoc `codesign -s -` in `packaging/macos/make-dmg.sh`, then `notarytool submit` + `stapler staple`.
- **Windows:** sign `pakpos.exe` and the installer with `signtool` (or Azure Trusted Signing) in the windows job.

## Windows code signing (SignPath)

Windows builds are signed through the [SignPath Foundation](https://signpath.org/)
once the project is accepted (see the [code signing policy](../code-signing-policy.md)).
`release.yml` already contains the steps; they are skipped until the repository
has the settings below, so releases keep working unsigned in the meantime.

One-time setup, after SignPath approves the project:

1. Install the SignPath GitHub App on `sansob/PakPos` (origin verification:
   SignPath checks that each request comes from this repository's release
   workflow on a tagged commit).
2. In SignPath, create the project (slug `PakPos`) with two artifact
   configurations, pasting the files from `.signpath/artifact-configurations/`:
   `executables` (pakpos.exe and pakpos-cli.exe) and `installer`
   (the Inno Setup installer). Use the signing policy `release-signing` with
   manual approval.
3. Create a CI user in SignPath and copy its API token.
4. In GitHub → Settings → Secrets and variables → Actions:
   - secret `SIGNPATH_API_TOKEN`: the token;
   - variable `SIGNPATH_ORGANIZATION_ID`: your SignPath organization ID;
   - optional variables `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_SIGNING_POLICY`
     if you chose other slugs than `PakPos` and `release-signing`.
5. Update the status line at the top of `docs/src/code-signing-policy.md` and the
   "not code-signed" notes in `installation.md` and the READMEs.

During a release the Windows jobs then wait (up to an hour) for you to approve
two signing requests per architecture in SignPath: the executables, then the
installer built from the signed executables. A last step fails the job if any of
them is not validly signed.

The uninstaller that Inno Setup writes (`unins000.exe`) is not signed: it is
generated inside the installer build, after the executables come back from
SignPath. Signing it too would need a SignPath call from Inno Setup's `SignTool`
hook (one more approval per build).
