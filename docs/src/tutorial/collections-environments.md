# 2. Collections, environments & variables

Hard-coding `https://staging.example.com` into every request does not scale.
Variables fix that.

## Collections and folders

A **collection** groups related requests, for example everything for one API.
Inside it you can create **folders** (and folders inside folders).

- Click **+** in the sidebar to create a collection.
- Right-click a collection or folder for *Add request*, *Add folder*, *Rename*,
  *Run collection*, *Export* and *Delete*.
- Right-click a request to *Duplicate*, *Rename* or *Delete* it.
- Use the search box to filter by name or URL.

## Environments

An **environment** is a named set of variables, such as *Local*, *Staging* and *Production*.

![Environments](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/environments.png)

1. Go to the sidebar's **Environments** tab and click **+ New**.
2. Rename it *Staging* and add a variable `base_url = https://httpbin.org`.
3. Pick *Staging* in the **Environment** dropdown in the header.

Now write URLs like:

```text
{{base_url}}/get
```

Variables are highlighted **green** when they resolve and **red** when they do
not; unresolved names are also listed under the URL bar, so typos are obvious.

Switching the environment dropdown changes every request at once. That is the
whole point.

## Where variables can be used

Everywhere you type text: URL, params, headers, body, auth fields, form fields,
and even the *expected* values of tests.

## Scopes and priority

When the same name exists in several places, the first match wins:

1. values extracted during a run (see the next tutorial),
2. the **active environment**,
3. **collection variables**,
4. **globals** (sidebar → Environments → *Globals*),
5. **dynamic variables**.

## Dynamic variables

| Variable | Example |
|---|---|
| `{{$guid}}` / `{{$randomUUID}}` | `3f0c…` (UUID v4) |
| `{{$timestamp}}` | `1760425200` (Unix seconds) |
| `{{$isoTimestamp}}` | `2026-10-14T07:00:00Z` |
| `{{$randomInt}}` | `0`–`1000` |
| `{{$randomBoolean}}` | `true` / `false` |
| `{{$randomEmail}}` | `user4821@example.com` |

## Inherited auth

Set auth once on a collection or folder and leave requests on **Inherit from parent**.
The Auth tab tells you which auth will actually be used.

Next: [Tests and request chaining](tests-chaining.md).
