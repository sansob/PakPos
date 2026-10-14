# Authentication

Pick the auth type on the **Auth** tab of a request, folder or collection.
Requests set to *Inherit from parent* use the nearest folder or collection
that has one. Every field accepts `{{variables}}`, so secrets can live in an
environment instead of the collection file.

| Type | What PakPos sends |
|---|---|
| Basic | `Authorization: Basic base64(user:password)` |
| Bearer | `Authorization: Bearer <token>` |
| API key | A header or query parameter of your choice |
| OAuth 2.0 | A token fetched and refreshed for you, see [OAuth 2.0](oauth2.md) |
| Digest | The answer to the server's `401` Digest challenge |
| NTLM | An NTLMv2 handshake |
| AWS Signature | AWS Signature Version 4 headers |

## Digest

PakPos sends the request once, reads the `WWW-Authenticate: Digest …`
challenge from the `401` response and repeats the request with the answer.
Supported: `MD5`, `SHA-256`, `SHA-512-256`, their `-sess` variants, and
`qop=auth` / `auth-int` (RFC 7616, compatible with RFC 2617 servers).

## NTLM

For Windows servers (IIS, SharePoint, Exchange, intranet APIs). PakPos runs
the three-step NTLMv2 handshake on one keep-alive connection. Write the user
as `DOMAIN\user` or fill the *Domain* field. NTLMv1, Kerberos and
signing/sealing are not supported.

## AWS Signature (SigV4)

Signs the request with your access key and secret: `X-Amz-Date`,
`Authorization` and, for temporary credentials, `X-Amz-Security-Token`.
Set *Region* (e.g. `ap-southeast-3`) and *Service* (`execute-api` for API
Gateway, `s3`, `lambda`, …). For `s3` the payload hash is sent in
`X-Amz-Content-Sha256`; multipart bodies are signed as `UNSIGNED-PAYLOAD`.
The signature includes the time, so it is computed again on every send.

## Client certificates (mTLS)

Some servers ask the client for a certificate. Add one in
**Settings → Client certificates**:

| Field | Example |
|---|---|
| Host | `api.internal.example`, `*.internal.example`, `localhost:8443` |
| Certificate | `client.pem` (may also contain the chain and the key) |
| Key | `client.key`, if the key is in a separate file |
| CA | `company-ca.pem`, to trust a private CA for that host |

The first enabled entry whose host matches is used, for HTTPS requests and
for gRPC over TLS. An entry with only a CA makes PakPos trust a private CA
without sending a certificate.

Files must be PEM. To convert a `.pfx` / `.p12`:

```sh
openssl pkcs12 -in client.pfx -out client.pem -nodes
```

From the command line, `--cert`, `--key` and `--cacert` work for `run`,
`send` and `grpc`; certificates from the app's settings are used too.

```sh
pakpos-cli send GET https://api.internal.example/health --cert client.pem --key client.key
```
