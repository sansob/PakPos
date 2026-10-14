# Security policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please **do not** open a public issue. Use
[GitHub private vulnerability reporting](https://github.com/sansob/PakPos/security/advisories/new)
with steps to reproduce and the affected version. You should get a reply within
a week. Fixes are released as soon as possible and credited unless you prefer otherwise.

## Scope notes

- PakPos stores collections and environments, including any secrets you type,
  as **plain JSON** in your workspace folder. Protect that folder like any other
  credentials file.
- `pakpos-cli mcp` lets an AI agent send HTTP requests. Run it with `--allow-host`.
- Turning off TLS verification is meant for local development only.
