# Testing MCP servers

PakPos can also be the *client* of a [Model Context Protocol](https://modelcontextprotocol.io)
server, so you can test a server the way you test a REST API. (To let an AI
agent use PakPos, see the [MCP server](mcp.md) instead.)

Pick **MCP** in the method picker. The URL bar holds the server:

| Server | URL bar |
|---|---|
| Streamable HTTP | `https://example.com/mcp` (headers and auth from the request apply; the session id is handled) |
| stdio | A command line that starts it: `npx -y @modelcontextprotocol/server-everything`, `uvx mcp-server-time`, `"C:\Program Files\srv.exe" --stdio` |

On the **Call** tab choose what the request does:

- **Call tool**: the tool name and its JSON arguments;
- **Read resource**: a resource URI;
- **Get prompt**: the prompt name and its arguments;
- **List tools**: everything the server offers.

**⟳ Load** connects and lists the tools, resources or prompts; picking one fills
the name and an arguments template generated from the tool's input schema.
For stdio servers you can add environment variables (API keys…) for the
server process.

**Send** connects, initializes, does the call and shows the result as the
response body. Status is `200` for a result, `500` when a tool reports
`isError`, `400` for a JSON-RPC error and `502` when the server cannot be
reached, so the usual tests and extractions work:

| Target | Example |
|---|---|
| `status` | *equals* `200` |
| `json.content[0].text` | *contains* `Jakarta` |
| `json.structuredContent.celsius` | *less than* `40` |

MCP requests run in the collection runner and `pakpos-cli run` too.

![MCP request](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/mcp.png)

## Trust

A stdio server is a program the request starts. Saved requests only start one
when their collection is **trusted** (the same trust as for
[scripts](scripts.md)); `pakpos-cli run` needs `--allow-scripts` for them.
HTTP servers follow the `--allow-host` list in `pakpos-cli mcp`.

## From the terminal

```sh
pakpos-cli mcp-client "npx -y @modelcontextprotocol/server-everything"            # list tools
pakpos-cli mcp-client http://127.0.0.1:8787/mcp call weather --args '{"city":"Bandung"}'
pakpos-cli mcp-client "uvx mcp-server-time" resources
pakpos-cli mcp-client https://mcp.example.com/mcp -H 'Authorization: Bearer …' prompts
```

`call` exits with code `1` when the tool reports an error, so it fits in scripts.

Not supported yet: the older HTTP+SSE transport, server-to-client requests
(sampling, elicitation, roots: they are answered with "not supported"), and
progress notifications.
