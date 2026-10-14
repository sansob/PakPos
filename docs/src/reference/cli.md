# Command-line interface

```text
pakpos-cli [--no-color] <COMMAND>

Commands:
  run        Run every request of a collection and its tests
  send       Send a single request, like curl but with PakPos variables
  import     Convert a Postman collection into PakPos format
  export     Export a PakPos collection as Postman v2.1
  curl       Convert a cURL command into a PakPos request (JSON) or a code snippet
  ws         Open a WebSocket: stdin lines are sent, messages are printed
  mock       Serve the saved examples of a collection as a mock API
  workspace  Print the workspace location and its contents
  mcp        Start a Model Context Protocol (MCP) server on stdio
```

`pakpos-cli <command> --help` shows every option. Colours are disabled when
output is not a terminal or `NO_COLOR` is set.

## run

```text
pakpos-cli run <COLLECTION> [-e ENV] [--var k=v]... [--filter TEXT] [--bail]
               [--delay MS] [--timeout S] [-k] [-r cli|json|junit] [-o FILE] [--save-env]
               [--allow-scripts]
```

`COLLECTION` and `ENV` are file paths (PakPos or Postman JSON) or names from the
workspace. Exit code `0` = all passed, `1` = failures, `2` = error.

## send

```text
pakpos-cli send <METHOD> <URL> [-H 'Name: value']... [-d BODY | -d @file]
                [-e ENV] [--var k=v]... [-i] [-k] [--timeout S]
```

The status line and headers (`-i`) go to stderr and the body to stdout, so
`pakpos-cli send GET … | jq` works. Exit code `1` for HTTP status ≥ 400.

## import / export

```text
pakpos-cli import <postman.json> [-o out.json] [--workspace]
pakpos-cli export <collection.json> [-o out.postman_collection.json]
```

## curl

```text
pakpos-cli curl "<curl command>" [--to curl|python|javascript|httpie]
```

## ws

```text
pakpos-cli ws <URL> [-H 'Name: value']... [-e ENV] [--var k=v]... [-s MESSAGE]... [--timeout S]
```

See [WebSocket](websocket.md).

## mock

```text
pakpos-cli mock <COLLECTION> [-p PORT] [--host 127.0.0.1] [--delay MS]
```

See [Mock server](mock.md).

## mcp

```text
pakpos-cli mcp [--allow-host HOST]... [--allow-write] [--allow-scripts]
```

See [MCP server](mcp.md).

## Environment variables

| Variable | Effect |
|---|---|
| `PAKPOS_HOME` | Workspace folder (default: OS data dir) |
| `NO_COLOR` | Disable colours |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | Standard proxy settings |
