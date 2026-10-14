# 4. Importing collections and cURL

## Collection files (v2.0 / v2.1)

PakPos reads collections in the widely used Collection v2.0 / v2.1 JSON format.

1. Export the collection from the tool you used before. In Postman, for
   example: collection **⋯ → Export → Collection v2.1**.
2. In PakPos: **Import → Collection file (PakPos / v2.1)…** and pick the file
   (you can select several).

What is converted:

| In the file | In PakPos |
|---|---|
| folders, requests, methods, URLs, query params (incl. disabled) | ✅ |
| headers, descriptions | ✅ |
| raw / urlencoded / form-data / file bodies | ✅ |
| GraphQL body | ✅ GraphQL body (query + variables) |
| auth: noauth, basic, bearer, apikey (at any level) | ✅ |
| collection variables | ✅ |
| saved responses (examples) | ✅ served by the [mock server](../reference/mock.md) |
| pre-request & test **scripts** (collection, folder, request) | ✅ imported; they run after you [trust the collection](../reference/scripts.md#trust). `pm.sendRequest` and `require` are not available |
| OAuth 2.0 (client credentials, password, authorization code) | ✅ (implicit is imported as authorization code) |
| AWS, Digest, NTLM… auth | ❌ skipped, with a warning |

Export back with right-click → **Export (Collection v2.1)…**.

## Environments

Export the environment as JSON from your previous tool (in Postman:
**Environments → ⋯ → Export**), then in PakPos: **Import → Environment…**.

## cURL

Three ways:

- Paste the command straight into the **URL bar**.
- **Import → cURL command…** (or `Ctrl/⌘ + I`).
- From a terminal: `pakpos-cli curl "curl …"` prints a PakPos request.

![Import cURL](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/curl.png)

Supported flags include `-X`, `-H`, `-d/--data/--data-raw/--data-binary`,
`--data-urlencode`, `--json`, `-F/--form`, `-u`, `-G`, `-I`, `-A`, `-b`, `-e`
and `--url`, with single quotes, double quotes and `\\` line continuations
(what browsers produce with *Copy as cURL*).

## Code snippets

The **</>** button converts the current request (with variables resolved) to
cURL, Python `requests`, JavaScript `fetch` or HTTPie.

![Code snippet](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/snippet.png)
