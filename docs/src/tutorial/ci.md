# 5. Running collections in CI

`pakpos-cli run` executes a collection without the GUI. It accepts PakPos or
Postman collection files, or the *name* of a collection in your workspace.

```sh
pakpos-cli run examples/pakpos-demo.collection.json -e examples/httpbin.environment.json
```

```text
PakPos ▸ PakPos Demo  ·  env: Demo (httpbin)

✔ Basics / Get with query params  GET 200 312 ms · 298 B
    https://httpbin.org/get?lang=id&app=pakpos
    ✓ status equals 200
    ✓ json.args.lang equals id
...
PASSED  requests: 11  ·  assertions: 13/13  ·  2140 ms
```

Exit codes: `0` everything passed, `1` a request or assertion failed, `2` usage
or file error. So a failing API check fails the pipeline.

## Useful options

| Option | Meaning |
|---|---|
| `-e, --environment <file or name>` | Environment file (PakPos or Postman format) or workspace environment name |
| `--var key=value` | Override a variable (repeatable), e.g. secrets from CI |
| `--filter text` | Only requests whose folder/name contains `text` |
| `--bail` | Stop at the first failure |
| `--delay <ms>` | Wait between requests |
| `--timeout <s>` / `-k, --insecure` | Timeout / skip TLS verification |
| `-r, --reporter cli\|json\|junit` and `-o, --out <file>` | Report format and destination |
| `--save-env` | Write extracted variables back to the environment file |
| `--allow-scripts` | Run the collection's JavaScript scripts (sandboxed) |

## GitHub Actions

```yaml
name: API tests
on: [push, pull_request]
jobs:
  api:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install pakpos-cli
        run: |
          curl -sSL https://github.com/sansob/PakPos/releases/download/v0.1.0/pakpos-0.1.0-linux-x86_64.tar.gz | tar -xz
          sudo install pakpos-*/pakpos-cli /usr/local/bin/
      - name: Run API collection
        run: |
          pakpos-cli run api/collection.json -e api/staging.json \
            --var token=${{ secrets.API_TOKEN }} \
            --reporter junit --out api-report.xml
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: api-report, path: api-report.xml }
```

Replace `0.1.0` with the release you want; pinning a version keeps CI reproducible.

## GitLab CI

```yaml
api-tests:
  image: debian:stable-slim
  script:
    - apt-get update && apt-get install -y curl ca-certificates
    - curl -sSL https://github.com/sansob/PakPos/releases/download/v0.1.0/pakpos-0.1.0-linux-x86_64.tar.gz | tar -xz
    - ./pakpos-0.1.0-linux-x86_64/pakpos-cli run api/collection.json -e api/staging.json -r junit -o report.xml
  artifacts:
    reports:
      junit: report.xml
```

## Other commands

```sh
pakpos-cli send POST '{{base_url}}/post' -e staging -H 'X-Trace: 1' -d '{"a":1}' -i
pakpos-cli import exported.postman_collection.json --workspace   # add to the desktop app
pakpos-cli export collection.json -o collection.postman_collection.json
pakpos-cli curl "curl https://api.example.com -H 'Accept: application/json'" --to python
pakpos-cli workspace                                             # where is my data?
```

Full details: [CLI reference](../reference/cli.md).
