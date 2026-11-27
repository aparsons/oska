# ACME is a standard library client, the certificate authority is configuration, and the keys are `Secret` values

- **Status:** Proposed (2026-11-27), for spec #824 (#827). Needs a human merge. Research: [Let's Encrypt and ACME](../research/2026-11-lets-encrypt-acme.md).

## Decision

1. **ACME support is a standard library client, `std.acme`.** It is Oska code in `crates/oska-check/src/std/` ([ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md)), with no new language feature. The account and order state machine is a set of pure functions over bytes, so recorded CA transcripts test it with no capability. A thin driver takes a `HostConnect` for the CA, a clock, `Random.secure_bytes` and the caller's challenge responder. The program asks for certificates by calling the library; the runtime does not.
2. **The certificate authority is configuration.** The caller passes a directory URL and, optionally, External Account Binding credentials (key ID and an HMAC key that is a `Secret`). No CA, directory URL or root is hard-coded. A non-HTTPS directory is refused, except a loopback test CA.
3. **The account key and the certificate keys are `Secret` values** (see GLOSSARY.md, secret value), as is the EAB HMAC key. They are never printed, interpolated, logged or compared, and every `expose()` of one is an audit site. The library writes them only through a `FileWrite` confined to one directory.
4. **The account key algorithm is ES256** (ECDSA P-256 with SHA-256). Servers must accept it (RFC 8555 section 6.2). Oska already has P-256 over `Secret` values and SHA-256, and the algorithm stays legal in a FIPS mode (#569). Ed25519 and RSA signing are not built for ACME.
5. **`oska serve --acme` is deferred.** If it comes, it is a thin wrapper over `std.acme`, not a second implementation.

## Why

A program that serves HTTPS needs a certificate, and certificate lifetimes are falling (200 days in 2026, 100 in 2027, 47 in 2029), so it must renew its own. The users are AI agents that write servers in Oska and the humans who deploy them.

A library keeps the authority in the program's signature, where `oskac audit` can list it. A flag on the runtime would give every program that sets it network and filesystem authority that no signature shows. A configured CA keeps Oska from tying itself to one company: Let's Encrypt, ZeroSSL, Google Trust Services and a private step-ca differ only in directory URL and EAB. Account keys are long-lived and sign requests that spend rate limits and name domains, so they get the same protection as any other secret. ES256 is the one algorithm every server must accept, so one algorithm covers every CA, and the signing code to maintain, and to keep constant-time, is one scheme.

`oska serve --acme` is deferred because a useful one needs a public listener, a server HTTP/1.1 parser and a TLS 1.3 server, which the web services spec (#826) owns. ADR 0025 makes listening loopback-only until that spec amends it, and ADR 0012 excludes a TLS server. A flag built before them would have nothing to serve the challenge or the certificate with. The library needs none of them: the caller serves the HTTP-01 token from a listener of their own.

## Considered options

- **A built-in server flag (`oska serve --acme`) first.** Hides the authority, couples the client to a server that does not exist yet, and gives programs that do not use `oska serve` nothing. Rejected for now; deferred, not forbidden.
- **A hard-coded CA (Let's Encrypt only).** Simple, but it breaks for anyone who needs EAB, a private CA, or a second CA when one is down or rate-limited. It also bakes a vendor into the standard library. Rejected.
- **Ed25519 or RSA account keys.** Ed25519 is not required of servers by RFC 8555; RSA signing needs big-number arithmetic that is slower and harder to make constant-time. Rejected for the first release.
- **Shelling out to an external client (certbot, acme.sh).** Needs process authority, and the keys leave `Secret`. Rejected.

## Consequences

- **New authority is visible.** A program holds a `HostConnect` for its CA and a confined `FileWrite` for key storage. A later `Acme` capability that narrows to named domains is a separate decision, with its own ADR if it changes the attenuation order.
- **CA text is `Untrusted`.** Response bodies, problem documents, `Location` and `url` values are validated before use.
- **Signing is a prerequisite.** `std.ecdsa.sign` over a `Secret` P-256 key must be constant-time and must not repeat a nonce (RFC 6979 or the CSPRNG). The crypto is hand-written and unaudited, as ADR 0012 says, and the docs keep saying so.
- **Server-side ACME waits.** HTTP-01 on a public port 80, TLS-ALPN-01 and certificate reload need the web services spec (#826) first.
