# Test identity provider

Keycloak for the OAuth 2.0 integration tests (`crates/rt-core/tests/oauth2_idp.rs`) and for
trying the flows by hand. Official image `quay.io/keycloak/keycloak:26.8.2` (pinned in
`idp.sh`), development mode, in-memory database, realm imported from
`realm-requesttailor.json` at start.

```sh
testdata/idp/idp.sh start                       # http://localhost:8180, waits until ready
RT_IDP_URL=http://localhost:8180 cargo test -p rt-core --test oauth2_idp
testdata/idp/idp.sh stop
```

`RT_IDP_PORT` changes the port (default 8180, not 8080), `RT_IDP_CONTAINER` the container
name (default `rt-idp`). Without `RT_IDP_URL` the integration tests print `SKIPPED` and pass.
Admin console: http://localhost:8180/admin, user `admin`, password `admin`.

## Realm `requesttailor`

Access tokens live **10 seconds**, so the refresh test does not wait long. The device code
lives 600 s, polling interval 5 s. Endpoints under
`http://localhost:8180/realms/requesttailor/protocol/openid-connect/`: `auth`, `token`,
`auth/device`, `userinfo`.

| Client | Type | Flow | Secret |
| --- | --- | --- | --- |
| `rt-client-credentials` | confidential | client credentials (no refresh token) | `rt-client-credentials-secret` |
| `rt-password` | confidential | resource owner password | `rt-password-secret` |
| `rt-auth-code` | public | authorization code, PKCE optional (S256 or plain) | none |
| `rt-auth-code-s256` | public | authorization code, PKCE S256 enforced | none |
| `rt-device` | public | device authorization grant (RFC 8628), consent page | none |

Redirect URIs of `rt-auth-code`: `http://127.0.0.1:47901/callback`, `:47902/callback`,
`:8765/callback` (desktop app and `rt send`: the shell listens there itself), and
`http://localhost:5173/oauth-callback.html`, `http://127.0.0.1:5173/…`, `:5185/…` (web app).
`rt-auth-code-s256` allows `http://127.0.0.1:47901/callback` only.

User: `alice` / `alice-password`.

The secrets and the password are test values for this throwaway container only. SAML 2.0
bearer (RFC 7522) is not offered by Keycloak; it is covered by the unit tests against a fake
token endpoint (`crates/rt-core/src/auth/oauth2/tests.rs`).
