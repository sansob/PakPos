# OAuth 2.0

Choose **Auth → OAuth 2.0** on a request, folder or collection. Set it once on
the collection and leave requests on *Inherit*; every request then shares the
same token.

![OAuth 2.0](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/oauth.png)

## Grant types

| Grant | When to use | How the token is obtained |
|---|---|---|
| **Client credentials** | Machine-to-machine APIs | Automatically on **Send**, in the runner, CLI and MCP |
| **Password** | Legacy first-party apps (avoid for new systems) | Automatically, like client credentials |
| **Authorization code** (+ PKCE) | Logging in as a user (Google, Auth0, Keycloak, Entra ID, GitHub, …) | Click **Get new access token**: your browser opens, you log in, PakPos receives the redirect on `http://127.0.0.1:<port>/…` |

The implicit grant is deprecated by the OAuth 2.0 Security BCP and is not offered;
Postman collections using it are imported as authorization code.

## Fields

All fields accept `{{variables}}`, so keep secrets such as `{{client_secret}}`
in an environment.

| Field | Notes |
|---|---|
| Token URL | The token endpoint, e.g. `https://id.example.com/oauth/token` |
| Authorization URL | Authorization code only |
| Client ID / secret | The secret is optional for public clients (PKCE) |
| Scope, Audience | Sent when not empty (audience is used by Auth0 and others) |
| Redirect URI | Must be `http://127.0.0.1:<port>/path` or `http://localhost:<port>/path`. Register exactly this URI with your provider. Default `http://127.0.0.1:7777/callback` |
| PKCE | S256, on by default |
| Client authentication | *Basic header* (`client_secret_basic`) or *In body* (`client_secret_post`) |
| Header prefix | Default `Bearer`; the request gets `Authorization: <prefix> <token>` |

## Where tokens live

Tokens are stored in `tokens.json` in the workspace, **never in collection files**,
so committing a collection never leaks a token. Add `tokens.json` to `.gitignore`
if your workspace is a git repository.

A token is reused while it is valid. Expired tokens are refreshed with the
refresh token when the server issued one; otherwise client-credentials and
password tokens are fetched again automatically. For authorization code, click
**Get new access token** again when the login expires.

## CLI, CI and MCP

`pakpos-cli run` and the MCP server fetch client-credentials and password tokens
on their own and reuse tokens from the workspace's `tokens.json`. Authorization
code needs a browser, so log in once in the app; the CLI and MCP then use (and
refresh) that token. With `pakpos-cli mcp --allow-host`, the token URL must also
be on the allowlist.

```sh
pakpos-cli run api.json -e staging.json --var client_secret=$CLIENT_SECRET
```

## Trying it offline

The bundled mock API (`python3 scripts/mock_httpbin.py`) has `/oauth/token`,
`/oauth/authorize` (auto-approves and redirects) and `/oauth/me`. The demo
collection's **Auth → Profile (OAuth 2.0)** request uses them.
