# 4. Moving from Postman or cURL

## Postman collections

1. In Postman: collection **⋯ → Export → Collection v2.1**.
2. In PakPos: **Import → Postman / PakPos collection…** and pick the file
   (you can select several).

What is converted:

| Postman | PakPos |
|---|---|
| folders, requests, methods, URLs, query params (incl. disabled) | ✅ |
| headers, descriptions | ✅ |
| raw / urlencoded / form-data / file bodies | ✅ |
| GraphQL body | converted to a raw JSON `{query, variables}` body |
| auth: noauth, basic, bearer, apikey (at any level) | ✅ |
| collection variables | ✅ |
| pre-request & test **scripts** | ❌ skipped, with a warning; rewrite as [tests](../reference/tests.md) / extractions |
| OAuth 2, AWS, Digest… auth | ❌ skipped, with a warning |

Export back with right-click → **Export (Postman v2.1)**.

## Postman environments

Postman: **Environments → ⋯ → Export**. PakPos: **Import → Environment…**.

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
