# TLS, git and crypto are written in Oska

## Decision

The package manager fetches dependencies with a TLS client and a git client that are written in Oska, crypto primitives included. They are built after the native backend ([ADR 0011](0011-oska-generates-native-code-itself.md)) and after these language features:

- **`Bytes`.** A packed, value-semantic byte buffer in the prelude. It is copy-on-write and changed in place through `var`. It has slicing, big- and little-endian reads, and append. Bytes from a capability are `Untrusted<Bytes>`, under the same trust rules as text ([ADR 0006](0006-trust-is-a-flag-on-text.md)).
- **Bitwise operators on `U8`, `U32` and `U64`.** `^ & | ~ << >>`. A shift by the type's width or more panics. `+ - *` stay checked, and wrapping arithmetic and rotation are methods (`wrapping_add`, `rotl`). `Int` has no bitwise operators.
- **Secret integers.** `Secret<U8>`, `Secret<U32>` and `Secret<U64>`. `+ - *` wrap, because a panic on overflow would be a branch on the secret. They have the bitwise operators, `rotl` and `rotr`, and `ct_select` and `ct_eq`, which return `Secret<Bool>`. The checker rejects a secret integer in an `if`, `match` or loop condition, as an index or map key, in a division, and as a shift amount. `expose()` remains the only way out, and it is an audit site.

The scope is kept small:

- **TLS:** a TLS 1.3 client only.
  - Ciphers: AES-128-GCM and ChaCha20-Poly1305 with SHA-256, and SHA-384 for P-384 certificates.
  - Key exchange: X25519, falling back to P-256.
  - Certificate checks: ECDSA P-256 and P-384, RSA (PKCS#1 v1.5 and PSS) and Ed25519. The client offers Ed25519 (0x0807) in `signature_algorithms`, first in its list, because `std.ed25519` verifies it for releases anyway (#697; spec #228 had said both).
  - Not included: 0-RTT, session resumption, client certificates, TLS 1.2 and a server.
- **Root certificates:** a pinned copy of the Mozilla root store ships with oskac and is updated each release. Configuration can add a CA file.
- **git:** the smart HTTP protocol, version 2, fetch only, shallow at the tag's commit. Not included: push, an index or working tree, and SSH.
- **Assurance:** published known-answer vectors (RFC and NIST), Project Wycheproof's edge cases, and a live interoperability job against GitHub, GitLab and Codeberg.

## Why

TLS, X.509 and git packfiles are parsers of hostile input, which is the code Oska exists to make safe, and they are the classic source of memory-corruption bugs in C. Writing them in Oska means the checker sees them:

- trust tracking covers every byte from the network;
- the fetcher's authority is visible in its signature (`NetConnect` and a `DirWrite` for the cache);
- they are the language's largest test, and they leave behind libraries that real programs can use.

Rust is no stronger on constant time: nothing in its semantics guarantees it. In Oska, secret integers let the checker prove that no branch or memory access depends on a secret.

## Considered options

- **Shell out to the system's git.** Rejected by the maintainer.
- **Protocols in Oska, primitives as Rust intrinsics, now or for good.** Rejected: the goal is an all-Oska implementation, and the native backend removes the interpreter's speed penalty that motivated intrinsics.
- **Everything in Oska on today's interpreter.** Rejected as too slow: about 1–4 s per handshake and minutes for a 10 MB packfile.
- **Separate constant-time types (`CtU32`) or a `#[constant_time]` attribute.** Rejected: a second secrecy concept, or secrecy that doesn't follow the data.
- **Differential fuzzing against OpenSSL, statistical timing tests, an external audit.** Not adopted. Assurance rests on test vectors and interoperability.

## Consequences

- The crypto is hand-written and has not been audited. Its assurance is test vectors and interoperability, so a subtle bug that no vector covers can go unnoticed.
- Timing safety rests on the checker's rules and on the native backend not adding secret-dependent branches. Nothing measures it.
- Secret integers wrap with plain operators, while public integers need `wrapping_*` methods to wrap.
- `std.x509` decides two path rules as follows. Among candidate issuers of one name, the authority key identifier of a certificate and the subject key identifier of a candidate pick the issuer when both are present, and rule out a candidate without a signature check; a certificate with no key identifier falls back to the name alone. A server's certificate with no extended key usage extension is refused (`ExtendedKeyUsage`), because RFC 5280 leaves it unrestricted but the TLS server use needs `serverAuth`; a CA with no such extension is accepted, as it restricts nothing. A malformed key identifier or extended key usage extension is `Malformed`.
- Montgomery arithmetic exists in two copies on purpose (#701). `std.montgomery` serves `std.ecdsa` and `std.rsa` over public `List<U64>`, and may branch. `std.p256` keeps its own copy over `Secret<U64>`, which must not. Oska has no function that works on both a secret and a public integer. One shared copy would be the constant-time one: `ecdsa` and `rsa` would wrap their values as secrets and call `expose()` on the results, which adds audit sites to modules that have none and slows TLS signature checks. A fix to the Montgomery code is made in both copies.
