# Workspace file format

```text
<workspace>/
├── collections/<id>.json     one file per collection
├── environments/<id>.json    one file per environment
├── globals.json              global variables
├── history.json              last 200 sent requests
├── tokens.json               OAuth 2.0 tokens (keep out of git)
├── cookies.json              cookie jar (keep out of git)
└── settings.json             theme, HTTP settings, active environment
```

Files are written atomically (temp file + rename) and pretty-printed with a stable
key order, so they diff well. A corrupt file is skipped with a warning instead of
stopping the app.

## Collection

```json
{
  "id": "pakpos-demo",
  "name": "PakPos Demo",
  "description": "…",
  "auth": { "type": "bearer", "token": "{{token}}" },
  "variables": [{ "key": "version", "value": "v1", "enabled": true }],
  "items": [
    {
      "kind": "folder",
      "id": "…",
      "name": "Users",
      "auth": { "type": "inherit" },
      "items": [
        {
          "kind": "request",
          "id": "…",
          "name": "Create user",
          "method": "POST",
          "url": "{{base_url}}/users",
          "params": [{ "key": "notify", "value": "true", "enabled": false }],
          "headers": [{ "key": "Accept", "value": "application/json", "enabled": true }],
          "body": { "mode": "raw", "content": "{\"name\":\"Budi\"}", "language": "json" },
          "auth": { "type": "inherit" },
          "assertions": [{ "target": "status", "op": "equals", "expected": "201", "enabled": true }],
          "extractions": [{ "variable": "user_id", "source": "json.id", "enabled": true }]
        }
      ]
    }
  ]
}
```

- `url` never contains the query string; query parameters live in `params`.
- `protocol` is omitted for HTTP; `"websocket"` marks a WebSocket request (its raw
  `body` is the message draft) and `"grpc"` a gRPC call, with
  `grpc: { method, proto_files, import_paths }` and the JSON message in the raw `body`.
- `examples` is a list of saved responses (`name`, `status`, `headers`, `body`)
  served by the mock server.
- `scripts` (`pre_request`, `test`) holds JavaScript; it may also appear on
  folders and the collection.
- `body.mode`: `none`, `raw` (`language`: `json`, `text`, `xml`, `html`, `javascript`),
  `form_url_encoded` (`fields`), `multipart` (`fields` with `is_file`), `binary` (`path`),
  `graphql` (`query`, `variables` as JSON text, `operation_name`).
- `auth.type`: `inherit`, `none`, `basic` (`username`, `password`), `bearer` (`token`),
  `api_key` (`key`, `value`, `location`: `header` | `query`),
  `oauth2` (`grant`: `client_credentials` | `password` | `authorization_code`, `token_url`,
  `auth_url`, `client_id`, `client_secret`, `scope`, `audience`, `username`, `password`,
  `redirect_uri`, `use_pkce`, `client_auth`: `header` | `body`, `header_prefix`).

## Environment

```json
{ "id": "staging", "name": "Staging",
  "variables": [{ "key": "base_url", "value": "https://staging.example.com", "enabled": true }] }
```

## Sharing a workspace with a team

The easiest way is the built-in [Git sync](git.md). Manually:

```sh
export PAKPOS_HOME=~/code/my-api/.pakpos   # point the app and CLI at a repo folder
```

Commit `collections/` and the environments without secrets; add `history.json`,
`settings.json`, `tokens.json`, `cookies.json` and secret environments to `.gitignore`.

## Several workspaces

The workspace button in the header (left, next to the logo) lists your
workspaces. From there you can:

- switch to another one: pending edits are saved and the open tabs of each
  workspace are remembered, unsaved ones included;
- **New workspace…**: pick a name and a folder (an empty one, by default
  `Documents/PakPos/<name>`). It starts with your current theme, language and
  HTTP settings, but trusts no collection;
- **Open folder as workspace…**: use any existing folder, for example a
  `pakpos/` folder in a project repository;
- remove an entry with `×` (the folder is left alone).

The list is stored in `workspaces.json` in PakPos's config folder
(`~/.config/PakPos` on Linux, `~/Library/Application Support/dev.PakPos.PakPos`
on macOS, `%APPDATA%\PakPos\PakPos\config` on Windows), or in `$PAKPOS_CONFIG`
if set. `pakpos-cli` opens the active workspace; `PAKPOS_HOME` overrides both.

