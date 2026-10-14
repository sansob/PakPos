# Test & extraction language

Tests and extractions are data, not code: each one picks a **target** from the
response and, for tests, compares it with an **operator** and an **expected** value.

## Targets

| Target | Value |
|---|---|
| `status` | Status code, e.g. `200` |
| `time` | Response time in milliseconds |
| `size` | Body size in bytes |
| `body` | Whole body as text |
| `header.<Name>` | A response header, case-insensitive (`header.content-type`) |
| `json` | The whole JSON body |
| `json.<path>` | A value inside the JSON body |

### JSON paths

| Path | Selects |
|---|---|
| `json.token` | field `token` |
| `json.data.items[0].id` | `id` of the first item |
| `json[2].name` | `name` of the third element of a top-level array |
| `json["content-type"]` | a key with characters that are not allowed in a plain name |
| `json.$.data` | a leading `$` is accepted for JSONPath users |

Strings are compared without quotes; numbers, booleans, `null`, arrays and
objects are compared as their JSON text (`true`, `[1,2]`, `{"a":1}`).

## Operators

| Operator | Passes when |
|---|---|
| `equals` / `not equals` | Text is equal, or both sides are numbers and numerically equal (`200` = `200.0`) |
| `contains` / `not contains` | Substring check |
| `less than` / `greater than` | Both sides are numbers and compare as stated |
| `exists` / `not exists` | The target is present (no expected value) |
| `matches (glob)` | Glob match: `*` any text, `?` one character, e.g. `ord_*` |

A missing target fails every operator except `not exists`.

The expected value may contain `{{variables}}`, which are resolved before comparing.

## Extractions

An extraction stores `target`'s value into a variable:

```text
json.data.token  →  token
header.Location  →  created_url
```

In the desktop app the value goes into the active environment (or globals when
none is active). In `pakpos-cli run` it is available to the following requests,
and `--save-env` writes it back into the environment file.

## In JSON files

```json
"assertions": [
  { "target": "status", "op": "equals", "expected": "200" },
  { "target": "json.data.id", "op": "exists" },
  { "target": "time", "op": "less_than", "expected": "500", "enabled": false }
],
"extractions": [
  { "variable": "token", "source": "json.token" }
]
```

Operator names in files: `equals`, `not_equals`, `contains`, `not_contains`,
`less_than`, `greater_than`, `exists`, `not_exists`, `matches`.
