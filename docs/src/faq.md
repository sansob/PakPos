# FAQ

**Can PakPos replace my current API client?**
For most day-to-day work, yes: HTTP, GraphQL, WebSocket, gRPC, SSE,
Socket.IO, MQTT and MCP requests, collections, environments, OAuth 2.0, Digest,
AWS SigV4 and NTLM auth, client certificates, tests and scripts, data-driven
runs, mock servers, CI runs and team sharing (through git). The gaps are listed
in the [roadmap](roadmap.md), npm libraries in scripts being the main one.

**Can I run existing JavaScript tests?**
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
Builds are not notarised. On macOS 15 and later open *System Settings →
Privacy & Security* and click *Open Anyway*; on older versions right-click →
*Open*. Or run `xattr -dr com.apple.quarantine /Applications/PakPos.app`.

**Can I use PakPos in Bahasa Indonesia?**
Yes: Settings → Language. It follows the system locale until you pick one.

**The window is blank or crashes on Linux.**
PakPos needs OpenGL 2+. In VMs or remote sessions try `LIBGL_ALWAYS_SOFTWARE=1 pakpos`.

**Self-signed certificate errors?**
Settings → disable *Verify TLS certificates* (local development only), or
`pakpos-cli … -k`.

**Why the name?**
*Pak Pos* is Indonesian for "Mr. Post": the friendly mail carrier who delivers your
requests and brings the responses back.
