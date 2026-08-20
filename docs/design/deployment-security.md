# Deployment and authentication boundary

Eclipse is a local-first application for a developer or early tester on one machine. The supported
default is HTTP on `127.0.0.1:8000`. Set `bind_address` in `config/config.toml`, pass
`--bind-address`, or set `ECLIPSE_BIND_ADDRESS` to a non-loopback IP only when intentionally allowing
trusted LAN clients. Startup logs always report the effective address and whether the process is in
local or LAN opt-in mode.

Direct internet exposure is unsupported. Eclipse does not terminate TLS. Internet-adjacent access must
use a trusted, correctly configured reverse proxy that provides HTTPS, request-size/rate controls,
and access policy appropriate to the operator. Set both `https_reverse_proxy = true` and
`trust_proxy_headers = true` only when that proxy runs on the same host and connects to Eclipse from a
loopback address. Eclipse otherwise rejects forwarded headers. The proxy must preserve `Host`; browser
requests with `Origin` must match `Host` and the configured HTTP/HTTPS mode.

## Credentials and sessions

New and changed passwords are Argon2id PHC records using a random salt per password (19 MiB memory,
2 iterations, one lane). Existing PBKDF2 records retain their stored legacy salt and are upgraded
inside the successful-login transaction. Username changes do not change either format's salt, so
they cannot invalidate a password.

Login sessions are opaque random bearer values; only a SHA-256 digest and server-side metadata are
stored. Sessions expire after `session_ttl_seconds` (seven days by default), can be revoked, and all
sessions for an account are invalidated on password change. Session rows survive clean or unclean
restarts. Rotating the historical `secret_key` invalidates only pre-Milestone-6 encrypted tokens;
it does not invalidate the new opaque sessions. The migration intentionally requires users holding
old stateless tokens to sign in once so revocation can be enforced.

Browsers receive a `dim_session` cookie with `HttpOnly`, `SameSite=Lax`, and a finite `Max-Age`.
`Secure` is added only in explicit HTTPS reverse-proxy mode, so ordinary localhost HTTP remains
usable. Login responses continue to include the token during the typed-transport transition for
non-browser/API clients and websocket authentication. The frontend retains that compatibility token
only in tab-scoped session storage; normal HTTP requests can also authenticate with the cookie.

For non-loopback listeners, failed logins are conservatively limited per immediate peer address over
a one-minute window. This is deliberately local and in-memory: it resets at restart and is not a
distributed public-internet abuse system.

## State permissions and diagnostics

New configuration directories are mode `0700` and newly created configuration/database files are
mode `0600` on Unix. Existing installations and operator-managed media/cache permissions are never
recursively rewritten. Client-facing failures use request IDs and stable redacted messages; detailed
database, process, and filesystem diagnostics remain in structured local logs. Media APIs return
display filenames rather than absolute server paths.

Controls intentionally deferred include public-internet TLS termination, distributed rate limiting,
WAF/bot controls, account recovery/email verification, and multi-host session replication. They do
not fit Eclipse's supported deployment model.
