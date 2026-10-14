# FAQ

**Is PakPos a drop-in replacement for Postman?**
For sending requests, collections, environments, basic auth types, tests and CI
runs: mostly yes. For JavaScript scripts, OAuth 2, GraphQL/WebSocket/gRPC, mock
servers and team cloud features: not yet (see the [roadmap](roadmap.md)).

**Why not JavaScript tests like Postman?**
Embedding a JS engine adds size and complexity, and running someone else's
collection would mean running their code. Declarative tests cover most checks,
are easy to review in a diff, and run the same way in the app, the CLI and the
MCP server.

**Where is my data? Is anything uploaded?**
Nothing is uploaded; there is no telemetry. Data is in the workspace folder shown
in the status bar. See [Installation → Where is my data?](installation.md#where-is-my-data).

**Can my team share collections?**
Yes: point `PAKPOS_HOME` at a folder in a git repository. Each collection is one
JSON file, so reviews and merges are straightforward.

**macOS says the app "cannot be opened".**
Builds are not notarised yet. Right-click → *Open*, or run
`xattr -dr com.apple.quarantine /Applications/PakPos.app`.

**The window is blank or crashes on Linux.**
PakPos needs OpenGL 2+. In VMs or remote sessions try `LIBGL_ALWAYS_SOFTWARE=1 pakpos`.

**Self-signed certificate errors?**
Settings → disable *Verify TLS certificates* (local development only), or
`pakpos-cli … -k`.

**Why the name?**
*Pak Pos* is Indonesian for "Mr. Postman": the friendly person who delivers your
requests and brings the responses back.
