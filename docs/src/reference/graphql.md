# GraphQL

Pick **Body → GraphQL** on a request (usually `POST` to your `/graphql` URL).

![GraphQL](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/graphql.png)

- **Query**: the operation text. `{{variables}}` work here too, but prefer GraphQL variables.
- **Variables (JSON)**: a JSON object, e.g. `{ "id": "{{user_id}}" }`. PakPos variables
  are resolved first, then the text must be valid JSON.
- **Operation name**: optional, for documents with several operations.

PakPos sends the standard envelope with `Content-Type: application/json`:

```json
{ "query": "...", "variables": { ... }, "operationName": "..." }
```

## Schema explorer

**Fetch schema** runs an introspection query against the request's URL with the
same headers and auth (including OAuth 2.0). The explorer lists queries,
mutations and subscriptions with their arguments and types; click one to replace
the query with a starter operation that declares the variables and selects the
scalar fields. Servers that disable introspection in production return an error;
point the request at a staging URL to explore.

## Tests

GraphQL errors come back with HTTP 200, so check the body:

```text
json.errors            not exists
json.data.user.name    equals   Budi Santoso
```

## Postman

Postman GraphQL bodies are imported and exported in this format.
