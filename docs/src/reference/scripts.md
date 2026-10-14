# JavaScript scripts

For checks that the [declarative tests](tests.md) can't express, requests,
folders and collections can carry JavaScript with a `pm.*` API:

- **Pre-request script**: runs before the request is sent and can change it.
- **Post-response script**: runs after the response arrives; `pm.test(...)`
  results appear in the response's **Tests** tab next to the declarative tests.

Open a request's **Scripts** tab; collection scripts are under right-click →
**Edit collection… → Scripts**. Order: collection → folders → request, for both phases.

![Scripts](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/scripts.png)

## Trust

Scripts are code, and code from someone else's collection should not run just
because you opened it. So:

- Collections **you create in PakPos** are trusted automatically.
- Collections that come from **outside** (imported, or a shared workspace folder)
  are not. The Scripts tab shows a banner; click **Trust collection** after reading
  the scripts. Trust is stored in your `settings.json`, never in the collection file,
  so a collection cannot mark itself as trusted.
- `pakpos-cli run` runs scripts only with `--allow-scripts` (or for collections you
  trusted in the app); `pakpos-cli mcp` needs `--allow-scripts` *and* app trust.

## The sandbox

Scripts run in an embedded [QuickJS](https://bellard.org/quickjs/) engine
(modern JavaScript, ES2023) with:

- **no** file system, process, environment or network access: no `require`,
  `fetch`, `XMLHttpRequest` or `pm.sendRequest`;
- a 3-second time limit and a 64 MB memory limit per script;
- a fresh context per script, so nothing leaks between runs.

If you need data from another endpoint, make it a separate request and pass the
value with an [extraction](tests.md#extractions) or `pm.environment.set`.

## API

| API | Notes |
|---|---|
| `pm.test(name, fn)` | Records a passing test, or a failing one if `fn` throws |
| `pm.expect(value)` | Chai-style: `.to.equal`, `.eql`, `.deep.equal`, `.include`, `.a("array")`, `.property(k, v)`, `.lengthOf`, `.above` / `.below`, `.within`, `.oneOf`, `.match`, `.ok`, `.true`, `.null`, `.undefined`, `.exist`, `.empty`, `.not` … |
| `pm.response` | `.code`, `.status`, `.headers.get(name)`, `.responseTime`, `.responseSize`, `.text()`, `.json()`, `.to.have.status(n)`, `.to.have.header(k)`, `.to.be.ok` |
| `pm.request` | `.url` and `.method` (read/write), `.headers.add / upsert / remove / get`, `.body.raw` (read/write) |
| `pm.environment`, `pm.collectionVariables`, `pm.globals` | `.get`, `.set`, `.unset`, `.has`, `.toObject()`, `.replaceIn("{{x}}")` |
| `pm.variables` | Local to the current run; `.get` falls back to environment → collection → globals |
| `pm.info` | `.requestName`, `.eventName` |
| `console.log / info / warn / error` | Shown in the response's **Console** tab and in CLI output |
| Legacy | `tests["name"] = bool`, `responseBody`, `responseCode.code`, `responseTime`, `postman.setEnvironmentVariable` |

Values written with `set` are stored as strings. Changes to the
environment, collection variables and globals are saved.

## Example

```js
// Post-response
const json = pm.response.json();

pm.test("Status code is 200", () => pm.response.to.have.status(200));
pm.test("Returns the created order", () => {
  pm.expect(json).to.have.property("id");
  pm.expect(json.items).to.be.an("array").with.lengthOf(2);
  pm.expect(json.total).to.be.within(1, 1_000_000);
});

pm.environment.set("order_id", json.id);
console.log("created", json.id);
```

## Imported scripts

Imported collections keep their scripts (collection, folder and request
level). Most `pm.test` / `pm.expect` / variable scripts work unchanged. Not
available: `pm.sendRequest`, `pm.cookies`, `pm.vault`, `pm.visualizer`,
`pm.iterationData`, `require(...)` of npm libraries (`lodash`,
`moment`, `crypto-js`, `cheerio`, …) and asynchronous code that waits on timers.
