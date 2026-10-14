# MCP server

`pakpos-cli mcp` implements the server side of the
[Model Context Protocol](https://modelcontextprotocol.io) over **stdio**:
newline-delimited JSON-RPC 2.0 on stdin/stdout, logs on stderr.

- Protocol versions: `2025-06-18` (preferred), `2025-03-26`, `2024-11-05`.
- Capabilities: `tools`. Methods: `initialize`, `ping`, `tools/list`, `tools/call`.
- Tool errors are returned as results with `isError: true`, so the model can read them.

## Options

| Option | Effect |
|---|---|
| `--allow-host <pattern>` | Repeatable. Only matching hosts may be contacted. Patterns: `api.example.com`, `*.example.com`, `localhost:8080`, `*`. Without it every host is allowed and a warning is printed. When set, redirects are not followed. |
| `--allow-write` | Adds the `save_curl_request` tool, which writes into the workspace. |

## Security model

- The host check runs **after** variables are resolved, on the final URL, for
  every request, including each request of `run_collection`.
- Environment **values** are never listed; only variable names are.
- Response data (bodies, headers) is returned to the client and thus to the model.
- The server never reads files outside the workspace, except multipart/binary
  bodies that a saved request explicitly references.

## Manual test

```sh
printf '%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
 | pakpos-cli mcp --allow-host httpbin.org
```
