# How could Oska have built-in support for Let's Encrypt and other ACME certificate services?

Date of access: 2026-11-27

Claims about the repo cite files, read on the same date. Claims about the world cite the source that owns them. "Inference:" marks my conclusions.

## Answer

Inference: build `std.acme` as an ordinary library, with the account key held as a `Secret`, and gate its authority by capabilities. Do not start with `oska serve --acme`. The ACME client is the small part. Oska has no way to sign, no TLS server, no way to listen on a public address and no `POST`. Those four come first. HTTP-01 on port 80 needs only a plain HTTP listener, so the first useful milestone needs no TLS server at all.

## What the repo has today

- TLS is a 1.3 client only. ADR 0012 lists "0-RTT, session resumption, client certificates, TLS 1.2 and a server" as not included ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)). `crates/oska-check/src/std/tls.osk` has `TlsConfig` with `with_alpn`, which is client-side ALPN, and an engine that handles ServerHello, not ClientHello ([tls.osk](../../crates/oska-check/src/std/tls.osk)).
- Signatures can only be verified. `std.ecdsa` has `verify` and `verify_der`, `std.ed25519` has `verify`, `std.rsa` has `verify_pkcs1` and `verify_pss`. None has a `sign` function ([ecdsa.osk](../../crates/oska-check/src/std/ecdsa.osk), [ed25519.osk](../../crates/oska-check/src/std/ed25519.osk), [rsa.osk](../../crates/oska-check/src/std/rsa.osk)). `std.p256` has public-key derivation and ECDH over `Secret` values, in constant time ([p256.osk](../../crates/oska-check/src/std/p256.osk)). It could back ECDSA signing, which needs a scalar multiply, a nonce and a modular inverse.
- Hashes and MACs: `std.sha2` has SHA-256, SHA-384 and SHA-512, and `std.hmac` has HMAC-SHA-256 over a `Secret` key ([sha2.osk](../../crates/oska-check/src/std/sha2.osk), [hmac.osk](../../crates/oska-check/src/std/hmac.osk)). That is what External Account Binding needs for HS256.
- Randomness: `Random.secure_bytes(n)` returns `Secret<Bytes>` ([prelude.osk](../../crates/oska-check/src/prelude.osk)). The FIPS note says the system random source is the runtime's `rt_sys_random` and that `Random` itself is SplitMix64, which is not cryptographically secure ([FIPS note](2026-11-fips-140-3.md)).
- X.509 parses certificates and builds paths. It has no CSR builder and no certificate writer. It has base64 for PEM, but it is private to `x509.osk` and the alphabet is the standard one, not base64url ([x509.osk](../../crates/oska-check/src/std/x509.osk)).
- `std.http` has `Request.get` only, with a client `Reader`. There is no `POST` and no server parser ([http.osk](../../crates/oska-check/src/std/http.osk)). `std.json` and `std.dns` exist.
- Listening is loopback-only on purpose: `NetListen.restrict_to_loopback(port)` is the only narrowing, and "no program becomes a network-facing server by accident" ([ADR 0025](../adr/0025-network-access-goes-through-host-narrowed-capabilities.md)). A public listener, or a `HostListen` capability, does not exist.
- Capabilities are the effect system, secrets are values that `==`, logging and interpolation refuse ([ADR 0002](../adr/0002-capabilities-are-the-effect-system.md), [GLOSSARY.md](../../GLOSSARY.md)). Trust is a flag on text ([ADR 0006](../adr/0006-trust-is-a-flag-on-text.md)). ADR 0012 says the crypto is hand-written and unaudited.
- No document in the repo mentions ACME or Let's Encrypt, except an eval task about SSRF in webhook delivery (`eval/tasks/ssrf-webhook-delivery/prompt.md`). Searched with grep over `*.md`.

## The protocol

Flow, from [RFC 8555](https://www.rfc-editor.org/rfc/rfc8555):

1. The client generates an account key and POSTs to `newAccount`. The server answers 201 with the account URL in `Location` (section 7.3).
2. The client POSTs `newOrder` with identifiers (7.4).
3. For each authorization the client fetches the challenges, makes the proof available, and POSTs to the challenge URL to say it is ready (7.5.1).
4. The client polls the order until it is `ready`, then POSTs a base64url DER CSR to `finalize`, whose identifiers must equal the order's (7.4).
5. The client downloads the certificate by POST-as-GET. The default is `application/pem-certificate-chain`, and alternates come as `Link` headers (7.4.2). Certificates are immutable, so renewal is a new order.

Request rules (RFC 8555, section 6): the body is a flattened JSON JWS with one signature; the protected header has `alg`, `nonce`, `url` and exactly one of `jwk` (new account, and revocation signed by a certificate key) or `kid` (the account URL); `alg` is never `none` or a MAC; the nonce comes from the server, and `badNonce` returns a fresh one; the content type is `application/jose+json`. Plain GET works only for the directory and `newNonce`. Everything else is POST-as-GET with an empty payload (6.3).

Challenge types:

| Type | Proof | Port | Wildcards | IP identifiers | Source |
|---|---|---|---|---|---|
| HTTP-01 | file served over HTTP | 80 only | no | yes | [LE challenge types](https://letsencrypt.org/docs/challenge-types/) |
| DNS-01 | TXT record | none (DNS) | yes | no | same |
| TLS-ALPN-01 | self-signed certificate in a handshake that negotiates `acme-tls/1` | 443 | no | yes | same, [RFC 8737](https://www.rfc-editor.org/rfc/rfc8737) |

- HTTP-01: Let's Encrypt follows up to 10 redirects, only to ports 80 or 443, and ignores certificate errors on HTTPS redirects ([LE challenge types](https://letsencrypt.org/docs/challenge-types/)).
- TLS-ALPN-01 certificate (RFC 8737, section 3): self-signed, exactly one `dNSName` SAN, a critical `acmeIdentifier` extension (OID 1.3.6.1.5.5.7.1.31) holding the SHA-256 of the key authorization, and ALPN exactly `acme-tls/1`. The CA connects with an SNI and closes after the handshake. Let's Encrypt says it suits large hosts and TLS reverse proxies, and client support is limited ([LE challenge types](https://letsencrypt.org/docs/challenge-types/)). Inference: it is the one challenge a TLS server could answer without a separate port 80 listener, and it needs the server-side TLS stack.
- Open: I could not read RFC 8555 sections 8.1, 8.3 and 8.4 (key authorization, the exact HTTP-01 path, the DNS-01 TXT name and value). The fetch tool cut the text off before them. The table above rests on Let's Encrypt's page, not on those sections.

## Account keys and JWS

- Compact JWS is `BASE64URL(header).BASE64URL(payload).BASE64URL(signature)`. ACME uses the flattened JSON form instead ([RFC 7515](https://www.rfc-editor.org/rfc/rfc7515), sections 3.1, 3.2; [RFC 8555](https://www.rfc-editor.org/rfc/rfc8555), section 6.2). base64url uses the URL-safe alphabet without padding (RFC 7515, section 2).
- Servers MUST implement ES256 and SHOULD implement EdDSA with Ed25519 ([RFC 8555](https://www.rfc-editor.org/rfc/rfc8555), section 6.2). ES256 is ECDSA over P-256 with SHA-256. Its signature is the 64-byte `R || S`, 32 bytes each with no leading zeros dropped, not DER ([RFC 7518](https://www.rfc-editor.org/rfc/rfc7518), section 3.4). RS256 is listed as Recommended there (3.1).
- Inference: Oska should use ES256. It is the algorithm every ACME server must accept. Oska already has P-256 arithmetic over secrets and SHA-256. It must add: a `std.ecdsa` `sign` (RFC 6979 deterministic nonce, or a nonce from the CSPRNG), a raw `R || S` encoder, and a constant-time scalar inverse. Ed25519 signing is the alternative, and it is a smaller algorithm with no nonce, but servers only SHOULD accept it. RSA signing would need a constant-time big-integer exponent. The repo's `std.montgomery` may branch on its inputs and `std.rsa` is verify-only, so RSA signing is the most work (ADR 0012, Consequences).
- FIPS mode: the FIPS note puts ECDSA P-256 and P-384, RSA and SHA-2 in the CAVP list and calls X25519 and Ed25519 questions it could not close ([FIPS note](2026-11-fips-140-3.md)). Inference: ES256 is also the choice that would stay legal in a FIPS mode.
- External Account Binding signs the account key with an HMAC key from the CA, as HS256. ZeroSSL and Google Trust Services need it (below). `std.hmac.sha256` already takes a `Secret` key.
- The account key must be a separate key from every certificate key ([LE integration guide](https://letsencrypt.org/docs/integration-guide/)).

## Renewal and ARI

- RFC 9773, published June 2025, adds a `renewalInfo` directory entry. An unauthenticated GET on `renewalInfo/<AKI keyIdentifier base64url>.<serial base64url>` returns a `suggestedWindow` (start and end) and an optional `explanationURL`. The client picks a random time in the window, honors `Retry-After`, and may name the old certificate in `replaces` on the new order ([RFC 9773](https://www.rfc-editor.org/rfc/rfc9773), sections 3, 4, 5).
- Let's Encrypt says: check ARI at least twice a day; as a backstop renew at one third of lifetime left, and at half for certificates valid under 10 days; retry with exponential backoff, at most once a day per certificate; keep certificates and keys in durable storage ([LE integration guide](https://letsencrypt.org/docs/integration-guide/)).
- Renewals coordinated through ARI are exempt from all Let's Encrypt rate limits ([LE rate limits](https://letsencrypt.org/docs/rate-limits/)).
- Inference: ARI needs the certificate's AKI and serial, and `std.x509` exposes `serial()` but the AKI as a getter is not in the public list I read. Adding it is small. The loop needs a clock and sleep, which Oska has as capabilities (not checked in detail).

## Let's Encrypt today

- Profiles ([LE profiles](https://letsencrypt.org/docs/profiles/)): `classic` (default, 90 days, DNS names only, 100 names), `tlsserver` (45 days, no Common Name, 25 names), `shortlived` (about 160 hours, DNS and IP identifiers, 25 names), and `tlsclient`, which is no longer available as of 2026-07-08. Let's Encrypt supports CRL and not OCSP. The directory lists the profiles that exist. The page defers the `newOrder` profile field to an IETF draft (draft-aaron-acme-profiles), so selection is not an RFC 8555 feature.
- Six-day and IP address certificates became generally available on 2026-01-15. IP certificates must be short-lived, and short-lived certificates are opt-in with no plan to become the default. Default lifetimes will fall from 90 to 45 days over the next few years ([ISRG announcement](https://isrg.org/post/ip-and-6day-general-availability/)). By the challenge page, HTTP-01 and TLS-ALPN-01 can prove an IP and DNS-01 cannot ([LE challenge types](https://letsencrypt.org/docs/challenge-types/)).
- Rate limits ([LE rate limits](https://letsencrypt.org/docs/rate-limits/)): new orders 300 per account per 3 hours; new certificates 50 per registered domain per 7 days; duplicates (same identifier set) 5 per 7 days, no override; authorization failures 5 per hour per identifier per account; 10 new accounts per IP per 3 hours (500 per IPv6 /48). Staging has far higher limits.
- Industry direction: the CA/Browser Forum Baseline Requirements cut the maximum validity of subscriber certificates to 200 days from 2026-03-15, 100 days from 2027-03-15 and 47 days from 2029-03-15, and domain-validation reuse to 10 days from 2029-03-15 ([CA/B Forum BR](https://cabforum.org/working-groups/server/baseline-requirements/requirements/)). Inference: renewal automation is not optional for a server Oska ships, and an `std.acme` that does not renew would be a trap. I did not read the later pages of the BR (from character 100000), so IP validation method details are open.

## Other ACME CAs

- ZeroSSL: directory `https://acme.zerossl.com/v2/DV90`. It requires External Account Binding, separate from its API key. Certificates are 90 days, free, wildcards supported, at most six subdomain labels ([ZeroSSL ACME docs](https://zerossl.com/documentation/acme/)). Which challenges it accepts is not on that page.
- Google Trust Services: production `https://dv.acme-v02.api.pki.goog/directory`, staging under `test-api.pki.goog`. EAB is required: a key ID and an HMAC, created with `gcloud publicca external-account-keys create`, usable within 7 days, one ACME account each. No charge, but there are quotas ([Google Public CA tutorial](https://docs.cloud.google.com/certificate-manager/docs/public-ca-tutorial)). Its supported key types are not on that page.
- Buypass: no longer an option. It stopped offering TLS certificates on 2025-10-15 and issued none after 2025-10-31. ACME certificates "will also be discontinued", with no separate date given ([Buypass](https://www.buypass.com/products/tls-ssl-certificates/discontinues-issuance-of-tls-ssl-certificates)).
- step-ca (Smallstep): a private CA with an ACME provisioner for http-01, dns-01, tls-alpn-01 (IPs and hosts) and device-attest-01. EAB is in the commercial software only ([Smallstep ACME basics](https://smallstep.com/docs/step-ca/acme-basics/)). Inference: step-ca is the right test target in CI, because it needs no public DNS and has no rate limits.
- Inference: a client that takes a directory URL and optional EAB credentials covers all of the above, so the CA is configuration. Hard-code none, since ADR 0012's pinned roots show how CA choices age.

## What a server-side TLS stack needs first

Inference, from the gaps listed above and the ADR 0012 scope:

1. A TLS 1.3 server handshake: ClientHello parse, ServerHello, key share (X25519 or P-256), `CertificateVerify` signing (this needs ECDSA signing again), Finished, ALPN selection, SNI-based certificate choice, and `acme-tls/1`. The client already has the record layer, AES-GCM, HKDF and the key schedule, so those are reusable.
2. Signing in `std.ecdsa` (and `std.ed25519` if wanted) over `Secret` keys.
3. A public listener capability. ADR 0025 made listening loopback-only on purpose. A `HostListen` narrowed to an address and port, with a listed attenuation in the authority report, keeps the rule that no program becomes a server by accident.
4. A server-side HTTP/1.1 request parser, `POST`, and a `Request` with a body for the client. The HTTP code is a hostile-input parser, so it needs the same limits as the client's.
5. CSR and self-signed certificate writers in `std.x509` (DER encoding of Name, SAN, extensions, signature), for finalize and for TLS-ALPN-01.
6. base64url, a JWK thumbprint (RFC 7638) and JWS encode in a public module. `std.json` exists for the bodies.

## Design options

A. `std.acme` library. Account and order state machine as pure functions on bytes, like the TLS engine (no capability, tests with recorded transcripts), plus a thin driver that takes `HostConnect` for the CA, a `Clock`, `Random`, and a place to write. The challenge responder is the caller's: a handler for HTTP-01 or a DNS callback for DNS-01. Fits ADR 0002 and 0012's style and is testable against step-ca.

B. Built-in `oska serve --acme`. Easiest for a user, but it makes a toolchain command a public server, and ADR 0025 says no program becomes network-facing by accident. It also bakes in a CA choice and storage. Inference: a thin wrapper over A later, if at all.

C. Capability-gated effect: an `Acme` capability that attenuates like `Net` (`Acme.restrict_to(domain)`), so a signature says which names a program may obtain certificates for, and `oskac audit` lists them. Inference: valuable, and consistent with ADR 0025. It is a layer on A. Without it a library can request a certificate for any name that the program can answer a challenge for, which is already limited by what the program controls.

D. Key storage as `Secret`. The account key and the certificate private keys should be `Secret<Bytes>` (the glossary rule: never displayed, interpolated, logged or compared; `expose()` is an audit site). They are persisted through a `FileWrite` confined to one directory (ADR 0008), with mode 0600 as a runtime concern. Let's Encrypt asks for durable storage to avoid re-issuance ([LE integration guide](https://letsencrypt.org/docs/integration-guide/)).

DNS-01 needs a DNS provider API, which differs by provider and holds broad credentials ([LE challenge types](https://letsencrypt.org/docs/challenge-types/)). Inference: leave it to a callback in the library and do not ship providers.

## Ranked recommendation

1. Add ECDSA P-256 signing and raw `R || S` (`std.ecdsa.sign`), base64url, JWS and JWK thumbprint, with RFC 7515 and RFC 7518 known-answer vectors and Wycheproof. This unblocks JWS and a server's CertificateVerify.
2. Add `POST` to `std.http`, a CSR writer to `std.x509`, and a `std.acme` client for HTTP-01 with ES256, ARI and EAB, tested against step-ca (design A, with D). No server TLS needed yet: the caller serves the token on a plain listener.
3. Add a narrowed public listener capability and a server-side HTTP parser (prerequisites 3 and 4), so an HTTP-01 responder is a few lines.
4. Add the `Acme` capability for authority reporting (design C).
5. Add the TLS 1.3 server, then TLS-ALPN-01 and an HTTPS server that reloads certificates when `std.acme` renews them.
6. Defer `oska serve --acme` (design B) and DNS provider integrations.

Missing Oska-side, confirmed by reading the repo: any signing function; a TLS server; a non-loopback listener; `POST` and a server HTTP parser; CSR and certificate writers; public base64url; a JWS and JWK module. Present: SHA-2, HMAC-SHA-256, secret P-256 arithmetic, `Random.secure_bytes`, `Secret` values, `std.json`, `std.dns`, a certificate parser, a TLS client engine.

## Open questions

- RFC 8555 sections 8.1, 8.3 and 8.4 were not readable with the tools used. They own the key authorization and the exact HTTP-01 and DNS-01 formats.
- The Let's Encrypt profile selection field is described only as an Internet-Draft. I did not read the draft.
- Which challenges ZeroSSL accepts, and which key types Google Trust Services accepts, are not stated on the pages read.
- I did not verify which `std.x509` getters exist beyond those listed above, nor the clock and sleep API.
- The CA/B Forum Baseline Requirements were read only to character 100000, so IP validation methods are not covered.
- Whether ES256 signing in Oska needs a deterministic nonce (RFC 6979, which I did not read) or a CSPRNG nonce is a design choice not settled here.

## Sources

1. [RFC 8555, ACME](https://www.rfc-editor.org/rfc/rfc8555) (sections 4 to 7 read, 8 not)
2. [RFC 8737, TLS-ALPN-01](https://www.rfc-editor.org/rfc/rfc8737)
3. [RFC 9773, ACME Renewal Information](https://www.rfc-editor.org/rfc/rfc9773)
4. [RFC 7515, JWS](https://www.rfc-editor.org/rfc/rfc7515)
5. [RFC 7518, JWA](https://www.rfc-editor.org/rfc/rfc7518)
6. [Let's Encrypt challenge types](https://letsencrypt.org/docs/challenge-types/)
7. [Let's Encrypt rate limits](https://letsencrypt.org/docs/rate-limits/)
8. [Let's Encrypt profiles](https://letsencrypt.org/docs/profiles/)
9. [Let's Encrypt integration guide](https://letsencrypt.org/docs/integration-guide/)
10. [ISRG: 6-day and IP address certificates generally available](https://isrg.org/post/ip-and-6day-general-availability/)
11. [CA/Browser Forum Baseline Requirements](https://cabforum.org/working-groups/server/baseline-requirements/requirements/)
12. [ZeroSSL ACME documentation](https://zerossl.com/documentation/acme/)
13. [Google Public CA tutorial](https://docs.cloud.google.com/certificate-manager/docs/public-ca-tutorial)
14. [Buypass discontinuation notice](https://www.buypass.com/products/tls-ssl-certificates/discontinues-issuance-of-tls-ssl-certificates)
15. [Smallstep step-ca ACME basics](https://smallstep.com/docs/step-ca/acme-basics/)
16. Repo: ADR 0002, 0006, 0008, 0012, 0025; `GLOSSARY.md`; `docs/research/2026-11-fips-140-3.md`; `crates/oska-check/src/std/{tls,ecdsa,ed25519,rsa,p256,sha2,hmac,http,x509}.osk`; `crates/oska-check/src/prelude.osk`.
