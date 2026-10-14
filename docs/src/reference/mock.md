# Mock server

Serve a collection's saved **examples** as a fake API, so a frontend or mobile
app can be built before the backend exists, or tests can run offline.

![Mock server](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/mock.png)

## 1. Save examples

Send a request, then click **＋ Example** in the response panel: status, headers
and body are stored on the request. Edit or add them in the request's
**Examples** tab (status code, name, headers, body). Examples from Postman
collections (`response[]`) are imported too.

## 2. Start the server

- **App:** right-click a collection → **Start mock server**. It listens on
  `http://127.0.0.1:8000` (or a free port if 8000 is taken); the status bar shows
  the URL, and the window lists the routes and every request it served.
- **CLI:**

  ```sh
  pakpos-cli mock "PakPos Demo"                       # workspace collection
  pakpos-cli mock api.postman_collection.json --port 4010 --delay 300
  pakpos-cli mock api.json --host 0.0.0.0             # reachable from your phone / LAN
  ```

## Routing

A request's method and URL **path** become a route; the host (or a leading
`{{base_url}}`) is ignored. Path segments written as `:id`, `{id}` or `{{id}}`
match anything, and routes with more fixed segments win (`/users/me` before
`/users/:id`).

| Saved request URL | Matches |
|---|---|
| `{{base_url}}/users/:id` | `GET /users/42`, `GET /users/abc` |
| `https://api.example.com/v1/orders` | `GET /v1/orders` |

Query strings and request bodies are not used for matching.

## Choosing an example

With several examples on a route, the first 2xx one is returned. Clients can ask
for another with a request header:

```sh
curl http://127.0.0.1:8000/post -X POST -H 'x-mock-response-code: 422'
curl http://127.0.0.1:8000/post -X POST -H 'x-mock-response-name: Validation error'
```

Every response carries `X-PakPos-Mock: <request> › <example>` and permissive CORS
headers, and `OPTIONS` preflights succeed, so browsers can call the mock directly.
Unknown routes return `404` with the list of available routes.
