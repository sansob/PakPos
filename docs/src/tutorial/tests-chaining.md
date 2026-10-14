# 3. Tests and request chaining

## Add tests

Open a request's **Tests** tab. Each row is one assertion:

```text
[x] status                 equals        200
[x] json.data.items[0].id  exists
[x] time                   less than     500
[x] header.Content-Type    contains      json
```

The presets (*Status is 200*, *Time < 1s*, *Is JSON*, *No "error"*) add common
checks in one click. After **Send**, the response pill shows *Tests 3/3* and the
response's **Tests** tab lists every result, with the actual value for failures.

![Tests](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/tests.png)

See the [test language reference](../reference/tests.md) for every target and operator.

## Chain requests with extractions

Typical login flow: `POST /login` returns a token, and later requests need it.

1. On the login request open **Extract** and add `json.token → token`.
2. On the other requests set **Auth → Bearer Token** to `{{token}}` (or set it
   once on the collection and let requests inherit it).
3. Send the login request. A toast confirms *Saved {{token}} to environment*:
   the value is written into the **active environment** (or into globals when no
   environment is active).

Extractions use the same targets as tests: `json.<path>`, `header.<Name>`,
`status`, `body`.

## Run a whole collection

Right-click a collection → **Run collection**. Requests run top to bottom with
the active environment; values extracted by earlier requests are available to
later ones.

![Runner](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/runner.png)

The same run works headless; see [Running collections in CI](ci.md).
