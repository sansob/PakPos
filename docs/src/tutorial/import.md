# 4. Importing collections, OpenAPI and cURL

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

## OpenAPI and Swagger

**Import → Collection or OpenAPI file…** also takes OpenAPI 3.0 / 3.1 and
Swagger 2.0 documents, in JSON or YAML. Every operation becomes a request:

- folders by the operation's first tag;
- the first server URL becomes the collection variable `{{baseUrl}}` (server
  variables use their defaults); path parameters become `{{variables}}` with
  their example values;
- query and header parameters with examples (required ones enabled), request
  bodies with an example built from `example`, `examples` or the schema, and
  `Accept` from the responses;
- auth from `securitySchemes` (Basic, Bearer, API key, OAuth 2.0), per
  collection and per operation;
- response examples as saved **examples**, so **Start mock server** serves the
  API right after the import.

Local `$ref`s are followed; webhooks, cookie parameters and OpenID Connect are
reported as warnings. A large spec like GitHub's REST API (1,200+ operations)
imports in well under a second.

Export works the other way: right-click a collection → **Export as OpenAPI
3…** writes OpenAPI 3.0.3 YAML (or JSON with a `.json` name), with schemas
inferred from the example bodies and response examples from saved examples.

```sh
pakpos-cli import petstore.yaml --workspace        # into the app's workspace
pakpos-cli run petstore.yaml -e staging            # run a spec directly
pakpos-cli export my.collection.json -f openapi-yaml -o openapi.yaml
```

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
