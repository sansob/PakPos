# Git sync

PakPos has no cloud. To share collections, put the workspace in a git
repository and use **⇅ Sync** in the header: your team gets review, history and
access control from GitHub/GitLab/Gitea, and nothing leaves the servers you choose.

![Git sync](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/git.png)

## Set up

**A new shared workspace:** open **⇅ Sync → Initialize repository**, paste the
repository URL (e.g. `git@github.com:team/api-collections.git`) into **Remote**,
click **Set**, then **Sync**.

**A workspace inside your project:** keep the collections next to the code:

```sh
export PAKPOS_HOME=~/code/payments-api/.pakpos   # macOS / Linux
setx PAKPOS_HOME %USERPROFILE%\code\payments-api\.pakpos   # Windows
```

PakPos only ever stages and commits files **inside the workspace folder**, so
your other uncommitted project changes are never swept into a PakPos commit.

## What gets shared

| Shared (committed) | Local only (`.gitignore`) |
|---|---|
| `collections/*.json`, `environments/*.json`, `globals.json`, `.gitignore` | `history.json`, `settings.json` (theme, trusted collections), `tokens.json` (OAuth tokens), `ui-state.ron` |

Keep real secrets out of shared environments: use `{{variables}}` and give each
person a local environment, or pass values with `--var` in CI.

## Buttons

| Button | Does |
|---|---|
| **Sync** | Commit your changes → pull (rebase) → push. The everyday button. |
| **Commit** | Commit workspace changes with the message you typed (default: "Update PakPos workspace"). |
| **Pull** | `git pull --rebase --autostash`, then reloads collections and environments. Open tabs with unsaved edits are kept. |
| **Push** | `git push` (sets the upstream on the first push). |

The header badge shows the number of changed files (refreshed every minute, and
every 15 seconds while the window is open).

## Authentication

PakPos runs your installed `git` with prompts disabled, so it uses whatever already
works in your terminal: SSH keys, the Git Credential Manager, `gh auth`, macOS
Keychain. If a push fails with an authentication error, run `git push` once in a
terminal inside the workspace to set credentials up.

## Conflicts

Each collection is one file, so two people editing different collections never
conflict. If the same collection was changed on both sides and git can't merge it,
PakPos aborts the pull and leaves your files untouched; resolve it in a terminal
(`git pull`, edit, `git rebase --continue`) or with your editor's git tools.
