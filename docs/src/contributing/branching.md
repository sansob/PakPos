# Branching model

A lightweight Git Flow:

```text
main      ──●────────────●──────────●─────   tagged releases only (v0.1.0, v0.2.0 …)
             \          / \        /
release/x.y   \        ●   \      /           stabilisation, version bump, changelog
               \      /     \    /
develop   ──●───●──●─●───●───●──●──●──────   integration branch, always builds
             \    /   \     /
feature/*     ●──●     ●───●                  one branch per feature / fix
```

| Branch | Purpose | Merges into |
|---|---|---|
| `main` | What users get. Every commit is a release, tagged `vX.Y.Z`. Protected. | — |
| `develop` | Integration of finished work. Default target for PRs. Protected. | `release/*` |
| `feature/<topic>` | New features (`feature/oauth2`) | `develop` |
| `fix/<topic>` | Bug fixes for the next release | `develop` |
| `release/<x.y>` | Freeze, version bump, changelog, last fixes | `main` and back into `develop` |
| `hotfix/<topic>` | Urgent fix for a released version, branched from `main` | `main` and `develop` |
| `docs/<topic>` | Documentation only | `develop` (or `main` for urgent doc fixes) |

## Rules

- Open pull requests against **`develop`** (except release/hotfix PRs into `main`).
- CI must be green; at least one approving review for `main` and `develop`.
- Squash-merge feature branches; use merge commits for `release/*` and `hotfix/*`
  so history shows what shipped.
- Never push directly to `main`.

## Setting it up on GitHub (maintainers)

```sh
git checkout -b main && git push -u origin main
git checkout -b develop && git push -u origin develop
```

Then in **Settings → Branches**, add protection rules for `main` and `develop`
(require PR, require status checks *fmt + clippy* and *test*), and set
`develop` as the default branch if you want PRs to target it by default.
Also enable **Settings → Pages → Source: GitHub Actions** for the docs site.
