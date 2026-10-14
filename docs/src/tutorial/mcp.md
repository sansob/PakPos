# 6. Using PakPos from AI agents (MCP)

`pakpos-cli mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio. MCP clients (Claude Desktop, Claude Code, Cursor, VS Code, …)
can then list your collections and environments, run saved requests, run
collections with their tests, and send ad-hoc requests.

> ⚠️ **Read this first.** An MCP server that sends HTTP requests lets an AI agent
> reach anything your machine can reach: `localhost`, your company network, cloud
> metadata endpoints. Always start it with `--allow-host` and list only the APIs
> the agent should talk to. With an allowlist, redirects are disabled so they
> cannot bounce the agent to another host.

## Configure your client

Most clients use the same JSON shape. Example for Claude Desktop
(`claude_desktop_config.json`) or Cursor (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "pakpos": {
      "command": "pakpos-cli",
      "args": ["mcp", "--allow-host", "api.staging.example.com", "--allow-host", "localhost:8080"]
    }
  }
}
```

Claude Code:

```sh
claude mcp add pakpos -- pakpos-cli mcp --allow-host api.staging.example.com
```

Use the full path to `pakpos-cli` if it is not on your `PATH`
(on macOS: `/Applications/PakPos.app/Contents/MacOS/pakpos-cli`).

The server reads the same workspace as the desktop app (or `PAKPOS_HOME`), so
anything you save in the GUI is immediately available to the agent.

## Try it

Ask your agent things like:

- "List my PakPos collections."
- "Run the *Login* request from *Payments API* with the Staging environment and show me the token field."
- "Run the whole *Payments API* collection and summarise which tests fail."
- "Save this curl command into the *Scratch* collection: `curl …`" (needs `--allow-write`).

## Tools

| Tool | What it does |
|---|---|
| `list_collections` | Names, ids and request counts |
| `list_environments` | Environment names and **variable names only** (values are never sent) |
| `list_requests` | Requests of a collection (method, URL, folder, number of tests) |
| `run_saved_request` | Execute a saved request with an environment, including its tests |
| `run_collection` | Run a collection; returns pass/fail per request and assertion |
| `send_request` | Ad-hoc request: method, URL, headers, body, environment |
| `save_curl_request` | Only with `--allow-write`: parse a cURL command and save it |

Responses are truncated at 100 000 characters to keep the agent's context small.

Note that response bodies, headers and resolved URLs are passed to the agent and
therefore to the model provider. Do not point it at APIs that return data you are
not allowed to share. See the [MCP reference](../reference/mcp.md) for protocol details.
