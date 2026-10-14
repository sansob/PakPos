# Cookies

PakPos keeps one cookie jar per workspace (`cookies.json`, git-ignored). Every
`Set-Cookie` a server sends, including on redirects, is stored, and matching
cookies go out with later requests: same domain (or a sub-domain for cookies
with a `Domain` attribute), a matching path, and HTTPS only for `Secure`
cookies. Expired cookies and `Max-Age=0` remove themselves.

**Cookies** in the top bar opens the jar: cookies grouped by domain with
path, expiry and flags. Values can be edited, cookies deleted one by one or
all at once, and new ones added for a domain.

A `Cookie` header written on a request is kept as is; stored cookies with
other names are appended to it.

## Runner and CLI

- The collection runner in the app starts with the workspace jar and saves
  what the run collected.
- `pakpos-cli run` starts every run with an empty jar, so CI runs don't depend
  on earlier ones; cookies set during the run are sent to later requests.
