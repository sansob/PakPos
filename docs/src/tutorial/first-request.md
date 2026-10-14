# 1. Your first request

This tutorial takes about five minutes. You will send a request, read the
response, and save it into a collection.

## The window at a glance

![Main window](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/overview.png)

| Area | Purpose |
|---|---|
| **Header** (navy bar) | *+ New* request, *Import*, the **environment picker**, theme toggle and settings. |
| **Sidebar** | *Collections* tree, *Environments* and *History*. |
| **Tabs** | Every open request is a tab. A dot (●) means unsaved changes. |
| **Request editor** | Method, URL, and the Params / Auth / Headers / Body / Tests / Extract / Docs tabs. |
| **Response panel** | Status, time, size, and the Body / Headers / Tests / Request tabs. Drag its top edge to resize. |

## Send a GET request

1. Click **+ New** (or press `Ctrl/⌘ + T`).
2. Keep the method on **GET** and type this URL:

   ```text
   https://httpbin.org/get?city=Jakarta
   ```

   Notice that the **Params** tab now shows `city = Jakarta`. The URL bar and the
   params table stay in sync: edit either one.
3. Press **Send** or `Ctrl/⌘ + Enter`.

The response panel shows a green **200 OK** pill, the time in milliseconds and
the size. The body is pretty-printed and highlighted because it is JSON; switch
to **Raw** to see it exactly as received.

> **No internet?** Run the bundled mock API: `python3 scripts/mock_httpbin.py`
> and use `http://127.0.0.1:8787/get?city=Jakarta` instead.

## Send a POST with a JSON body

1. Change the method to **POST** and the URL to `https://httpbin.org/post`.
2. Open **Body**, choose **raw**, keep **JSON** and type:

   ```json
   { "name": "Budi", "city": "Jakarta" }
   ```

   `Content-Type: application/json` is added automatically. Use **Beautify** to
   re-indent, and watch for the *Invalid JSON* hint while typing.
3. **Send**. httpbin echoes your JSON back under `"json"`.

Open the response's **Request** tab to see exactly what was sent, including the
final URL and all headers (the `Authorization` value is masked).

## Save it

Press **Save** (`Ctrl/⌘ + S`). Because the request is new, PakPos asks for a name
and a collection; pick **New collection…**, call it *My first API*, and save.
The request now appears in the sidebar. Later saves happen in place.

## Tips

- Paste a whole `curl …` command into the URL bar: it is converted into a request.
- **History** keeps the last 200 requests you sent; click one to reopen it.
- **</>** opens code snippets (cURL, Python, JavaScript, HTTPie) for the current request.

Next: [Collections, environments & variables](collections-environments.md).
