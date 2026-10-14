# FAQ

**Is PakPos a drop-in replacement for Postman?**
For most day-to-day work, yes: HTTP, GraphQL, WebSocket and gRPC requests,
collections, environments, OAuth 2.0, tests and scripts, mock servers, CI runs and
team sharing (through git). The gaps are listed in the [roadmap](roadmap.md):
`pm.sendRequest` and npm libraries in scripts, data-driven runs, some auth types.

**Does PakPos run Postman's JavaScript tests?**
Yes, in a sandbox (no file, process or network access) and only for collections
you trust; see [JavaScript scripts](reference/scripts.md). For new work, the
declarative tests are easier to review in a diff and need no trust at all.

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
