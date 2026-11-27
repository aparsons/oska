# How could Oska build and host AI-first, secure-by-default network and web services, and the elements of a modern web application?

Date of access: 2026-11-27

Scope: what Oska has today (read from the repo, not run), the gaps, options per web-app element, a phased path, and security-by-default choices. "Inference:" marks my own conclusions. "Not verified" marks repo claims I did not check by reading code or running tests. Web pages were read through a summarizing fetcher, so the quoted details come from those summaries. Where a page excerpt was truncated, the file says so.

## 1. What Oska has today

All paths are relative to `/home/user/oska`. I read these files and did not run any code.

- **Capabilities are the effect system.** A function's effects are the capabilities it can reach. `docs/adr/0002-capabilities-are-the-effect-system.md`.
- **Trust is a flag on text.** Plain `String` is trusted. `Untrusted<String>` flows only into untrusted slots. Prelude text ops propagate untrust. `.assume_trusted()` is an audit site. `docs/adr/0006-trust-is-a-flag-on-text.md`.
- **Classification ladder.** The levels are `Public < Internal < Confidential < Restricted`. Sinks have ceilings, and only a `Declassify` capability lowers a level. Logging, network send and `std.llm` come first. `docs/adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md`. This is an accepted decision. Not verified: whether the checker implements it yet.
- **Network capabilities.** `NetConnect.restrict_to(host, port)` gives a `HostConnect`. `NetListen.restrict_to_loopback(port)` gives a `LoopbackListen`. Listening is loopback-only, "for the local test server". Everything read from a connection is `Untrusted`. Network-facing listening is not available. `docs/adr/0025-network-access-goes-through-host-narrowed-capabilities.md`. `LoopbackListen.accept` exists in the runtime source: `crates/oska-check/src/std/rt.osk` (around line 7532).
- **TLS is a client only.** It is TLS 1.3 only, with no server, no 0-RTT, no resumption and no client certificates. It is written in Oska. `docs/adr/0012-tls-git-and-crypto-are-written-in-oska.md` and `crates/oska-check/src/std/tls.osk`. `TlsConfig` carries an ALPN list (`tls.osk` line 130).
- **Crypto in Oska.** The files are `aes128gcm`, `chacha20poly1305`, `hmac`, `hkdf`, `sha2`, `x25519`, `p256`, `ecdsa`, `ed25519`, `rsa`, `x509` and `x509_roots`, all in `crates/oska-check/src/std/`.
- **HTTP is an HTTPS GET client.** `std.http` is HTTP/1.1 only and does no redirects, compression, cookies or keep-alive. It has a strict response parser (the `Reader` struct) and rejects header injection. `crates/oska-check/src/std/http.osk` (header comment, lines 1-60). There is no HTTP server, HTTP/2 or HTTP/3.
- **HTML builder.** `std.html` has `Html`, `html.text` (escapes), `TrustedHtml.new` (trusted text only) and `render`, which returns `Untrusted<String>`. It has a signature-only declaration with no template engine. `crates/oska-check/src/std/html.osk`.
- **JSON and schema.** `std.json` has `parse`, `encode`, `decode` and a `Schema` in the JSON Schema subset that MCP and Claude structured outputs take. `crates/oska-check/src/std/json.osk` (lines 24-27, 97-121).
- **MCP server over stdio.** `std.mcp` has `Toolset` and `serve`, and it speaks the 2026-07-28 and 2025-11-25 eras. Tool annotations (`readOnlyHint`, `destructiveHint`, `openWorldHint`) are derived from the capabilities the checker found, and no program can set them. `crates/oska-check/src/std/mcp.osk` (header), `GLOSSARY.md` lines 85-98, `examples/notes_server.osk`. There is no HTTP transport.
- **SQL.** `std.sql` checks only. `oskac run` refuses it (`E0371`). `crates/oska-check/src/std/sql.osk` (lines 1-6).
- **Site generator in Oska.** `site/build.osk` builds oskalang.dev with `std.html`, with narrowed filesystem capabilities. It writes Markdown copies, `llms.txt`, `llms-full.txt`, `sitemap.xml` and `robots.txt`. `site/build.osk` (header, lines 1-46). Not verified: that the pipeline runs today.
- **No JS, CSS or WASM tooling.** I found no JS or CSS support in the checker or std. Design notes say WASM component imports are "Later", as the first FFI form: `docs/design.md` lines 524 and 571. ADR 0011 rejects a WASM engine, whether Oska's own or an external one, as the way to run Oska programs. `docs/adr/0011-oska-generates-native-code-itself.md`. Not verified: the claim of no JS or CSS support rests on a grep of a few docs. I did not search every crate.
- **Native backend.** Oska generates its own native code (aarch64 and x86_64 Linux, macOS, Windows) and takes no outside crates. ADR 0011 and `docs/adr/0014-every-supported-platform-builds-and-runs-natively.md`.

## 2. What the standards require (primary sources)

- **MCP Streamable HTTP.** The server provides one endpoint that supports POST and GET. It MUST validate `Origin` and answer 403 if the header is invalid, SHOULD bind to localhost when run locally, and SHOULD authenticate. Session IDs in `MCP-Session-Id` SHOULD be cryptographically secure. The client sends `MCP-Protocol-Version`. https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- **MCP authorization (HTTP only).** An MCP server is an OAuth 2.1 resource server. It MUST implement Protected Resource Metadata (RFC 9728), either in a `WWW-Authenticate` `resource_metadata` parameter or at a well-known URI. It MUST validate that a token was issued for it as audience (RFC 8707) and MUST NOT pass tokens through to upstream APIs. Tokens go in the `Authorization` header and never in the query string. Stdio servers SHOULD NOT use this spec. https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization
- **llms.txt.** It is Markdown at `/llms.txt`. It has an H1, an optional blockquote summary and H2 link lists, and an `Optional` section. Pages may offer `.md` copies, with discovery through `rel="alternate" type="text/markdown"` or `Link:` headers. https://llmstxt.org/ (a proposal, not an IETF or W3C standard)
- **OpenAPI.** The current version on the site is v3.2.1 (10 September 2026). A document describes an HTTP API in JSON or YAML. The security-scheme list was truncated in my read of the page. https://spec.openapis.org/oas/latest.html
- **HTTP/2 (RFC 9113).** It uses 9-octet frame headers and multiplexed streams, and it has flow control. HPACK has stateful compression, and the ALPN token is `h2`. I did not read section 10 (DoS considerations), because my excerpt cut it off. https://www.rfc-editor.org/rfc/rfc9113.html
- **HTTP/3 (RFC 9114).** It runs on QUIC, uses ALPN `h3` and QPACK, and takes its handshake from TLS 1.3. Clients should fall back to TCP when UDP is blocked. https://www.rfc-editor.org/rfc/rfc9114.html
- **CSP.** The `Content-Security-Policy` header is the preferred delivery. Nonces match exactly. `strict-dynamic` lets trusted scripts load more scripts. `frame-ancestors` does not work in `<meta>`. https://www.w3.org/TR/CSP3/
- **Strict CSP recipe.** The recipe is `script-src 'nonce-RANDOM' 'strict-dynamic'; object-src 'none'; base-uri 'none'`. Nonces suit dynamic pages and hashes suit static pages. Roll out with `Content-Security-Policy-Report-Only`. The page does not say that nonces must be fresh per response, so that rule is not cited from this source. https://developer.mozilla.org/en-US/docs/Web/Security/Practical_implementation_guides/CSP
- **Trusted Types.** `require-trusted-types-for 'script'` makes strings passed to sinks such as `innerHTML`, `eval` and `script.src` throw a TypeError. `trusted-types` allowlists policy names. https://developer.mozilla.org/en-US/docs/Web/API/Trusted_Types_API
- **Security headers.** The OWASP-recommended values are: HSTS `max-age=63072000; includeSubDomains; preload`, `X-Content-Type-Options: nosniff`, COOP `same-origin`, CORP `same-site`, `Referrer-Policy: strict-origin-when-cross-origin`, Permissions-Policy with unneeded features disabled, and CSP `frame-ancestors` in preference to `X-Frame-Options`. https://cheatsheetseries.owasp.org/cheatsheets/HTTP_Headers_Cheat_Sheet.html
- **CORS.** `*` is not allowed with credentials, and credentialed responses need explicit origin, headers and methods plus `Access-Control-Allow-Credentials: true`. A specific origin needs `Vary: Origin`. Preflight is an `OPTIONS` request. Requests with `Authorization` are always preflighted. https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS. The WHATWG Fetch excerpt I got lacked the CORS protocol section itself (https://fetch.spec.whatwg.org/#cors-protocol), so I did not confirm the rules against it.
- **CSRF.** OWASP recommends the synchronizer token for stateful apps and the signed double-submit cookie (HMAC-SHA-256, constant-time compare) for stateless ones. SameSite is defense in depth only. Reject cross-site unsafe methods using `Sec-Fetch-Site`, fall back to Origin or Referer checks, and never allowlist `null`. https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html
- **Sessions.** Session IDs need at least 64 bits of entropy from a CSPRNG, and 128 bits for custom IDs. Cookies use `Secure`, `HttpOnly` and `SameSite`, with the `__Host-` prefix recommended. Regenerate the ID on a privilege change. Enforce idle and absolute timeouts on the server. https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html
- **WebAssembly.** WASM is validated and sandboxed. It has no inherent access to its surroundings, so all I/O goes through imports the embedder supplies. The embedder sets the policy by choosing what to import. The core spec does not define host interaction. https://webassembly.github.io/spec/core/intro/introduction.html

## 3. Gaps

Inference: from sections 1 and 2, these are the gaps, in rough order of blocking power.

1. **No server socket facing the network.** Listening is loopback-only by ADR 0025 and the TLS client has no server mode (ADR 0012). Nothing can host a public service today.
2. **No HTTP server.** `std.http` is a GET-only client. A server needs a request parser (reusing the strict `Reader` ideas), a response writer, keep-alive, timeouts, size limits and routing.
3. **No TLS server, HTTP/2 or HTTP/3.** A TLS server needs certificate loading, key handling (a `Secret`-typed key fits ADR 0012) and ALPN selection. HTTP/3 additionally needs QUIC, which is a large new protocol.
4. **No templates, cookies, sessions, CSRF, CORS or header helpers.** Only escaping HTML (`std.html`) exists.
5. **MCP is stdio only.** There is no Streamable HTTP, no OAuth resource-server role, and no OpenAPI emitter.
6. **No JS, CSS or asset pipeline, and no WASM.** ADR 0011 rules out a WASM engine, and design.md puts WASM imports in "Later".
7. **SQL has no runtime** (`sql.osk` header).

## 4. Options per web-app element

For each element I list options and then mark my choice as Inference.

### 4.1 HTTP server, TLS, HTTP/2 and HTTP/3

- **Option A: Oska-written server.** It is built the way ADR 0012 builds TLS and git: in Oska, under the checker, with trust tracking on every request byte. HTTP/1.1 first (RFC 9110 and 9112), then a TLS 1.3 server, then HTTP/2 (RFC 9113).
- **Option B: terminate TLS and HTTP/2/3 in an outside proxy.** Oska serves HTTP/1.1 on loopback behind Caddy or nginx. This needs no Oska work, but a non-Oska binary then carries all the hostile-parser risk. It also matches the loopback-only listener of ADR 0025.
- **Option C: skip HTTP/3.** QUIC is a transport that needs congestion control, loss recovery and its own TLS 1.3 integration (RFC 9114 says QUIC uses TLS 1.3 as the handshake).
- Inference: choose B for the first demo and A for the real service. Skip HTTP/3 until HTTP/2 is stable, and let a proxy cover it. The listener type should keep its name honest, as ADR 0025 intends: add a distinct `PublicListen` capability, narrowed by port and by host name, and never allow `main` to declare one by accident.

### 4.2 Routing and handlers

- Inference: model a route as a typed function whose parameters are the capabilities it needs, so the authority report lists each route's blast radius. `std.mcp` already works this way for tools (the `call` arms are listed per tool, `mcp.osk` header). Path and query parameters arrive as `Untrusted<String>`, and parsing them into `Int` or enum types makes them trusted by ADR 0006's rule that non-text results are trusted. A request body should use `json.decode` with a `Schema`.

### 4.3 HTML and templating

- **Option A: keep `std.html` as combinators.** It is already safe by construction (escaping by default, raw only through `TrustedHtml.new`). `site/build.osk` shows the pattern in use.
- **Option B: a template literal like `sql"..."`.** `std.sql` already restricts query text to a literal so that values enter as parameters only. The same shape for `html"<p>{name}</p>"` would be natural, with holes escaped by context (text, attribute, URL). This needs a checker feature (not verified how literals are implemented).
- Inference: start with A, which exists, and add B later. Context-aware escaping (attribute, URL, script) is the hard part and needs its own tests.

### 4.4 JavaScript

- **Option A: no JS by default.** Serve server-rendered HTML with a strict CSP that forbids scripts, and add progressive enhancement only where needed.
- **Option B: treat JS as a checked asset.** Oska serves static `.js` files and computes the SRI hash or CSP hash at build time. It does not parse the JS.
- **Option C: bundle and minify in Oska.** This means writing an ECMAScript parser (the language is defined by https://tc39.es/ecma262/, which I did not fetch for this report) and a minifier. It is a very large project, and a parser of complex hostile input is exactly the code Oska says it wants to write itself, so it is possible but low priority.
- **Option D: compile Oska to JS.** Rejected for now by ADR 0011's direction (native code only). Not verified: that no later ADR changes this.
- Inference: A first, B second. C only if users ask, and D not at all before a WASM or JS target decision.

### 4.5 CSS

- **Option A: static CSS files with a content hash in the name.** The CSP carries `style-src 'self'`.
- **Option B: a CSS builder in Oska.** Oska would parse and minify CSS, for example the W3C CSS Syntax grammar, which I did not fetch.
- Inference: A is enough. CSS cannot run scripts, but it can leak data through URLs (`url(...)`) and through selectors on attributes, so a build-time rule that CSS comes only from trusted text and forbids remote `url()` fits the trust model. This is my reasoning and I did not cite a source for the CSS leak vectors.

### 4.6 Asset bundling and minification

- Inference: the first step is a content-hash step in `site/build.osk` style: read files through a `DirRead`, write them through a `DirWrite`, name each by its hash, and emit a manifest that the HTML builder uses to produce `<script integrity=...>` or CSP hashes. Hashing needs `sha2` (present). Minification can wait. HTTP compression (gzip or brotli) is a separate decision, and `std.http` currently refuses it on purpose (`http.osk` header).

### 4.7 WebAssembly

- Two different questions: Oska hosting WASM, and Oska targeting WASM.
- **Hosting WASM modules as plugins.** WASM's own model fits Oska's: all host access is via imports the embedder chooses (https://webassembly.github.io/spec/core/intro/introduction.html), which maps onto capabilities. design.md already plans "WASM component imports" typed by the capabilities they need (`docs/design.md` line 524) and leaves the exact mapping to WASI as an open question (line 571). Running WASM needs an engine, which ADR 0011 rejects, so this stays open.
- **Serving WASM to browsers.** Oska can serve `.wasm` files as static assets with the right `Content-Type` and a CSP that allows them. That works today at the asset level.
- **Targeting WASM from Oska.** Not planned in any file I read. ADR 0011 says the earlier "WASM/WASI as the first compiled target" roadmap line was replaced.
- Inference: serve only. Defer engine and target decisions.

### 4.8 Sessions, auth, CSRF, CORS, headers

- **Sessions.** Server-side opaque IDs from `Random`, at least 128 bits, in a cookie with `__Host-`, `Secure`, `HttpOnly` and `SameSite=Lax` or `Strict`, regenerated at login, with server-enforced idle and absolute timeouts (OWASP session sheet above).
- **Auth.** For humans, passkeys or password hashing (not present in std: `sha2`, `hmac` and `hkdf` exist, but I found no password-hashing function such as Argon2 or scrypt, and I did not search for one beyond the file list). For agents, OAuth 2.1 bearer tokens per MCP authorization above. JWT verification would use `ed25519`, `ecdsa` or `rsa`, which exist.
- **CSRF.** Synchronizer token as the default for stateful apps, with a `Sec-Fetch-Site` check and an Origin check, plus SameSite as depth.
- **CORS.** Default to no CORS headers. Offer an explicit allowlist of exact origins and always add `Vary: Origin`. Reject `*` together with credentials at the API level, so the unsafe pair cannot be written.
- **Headers.** Send the OWASP set by default, including a strict nonce CSP and `frame-ancestors 'none'`.
- Inference: all of this should be one `std.web` layer whose defaults are on, so that turning one off is an explicit, auditable call.

### 4.9 AI-facing surfaces

- **MCP over HTTP.** Add the Streamable HTTP transport to `std.mcp`. The `Toolset` and `handle` code already exists, and `handle` is transport-independent (`mcp.osk` header). Required: `Origin` validation, protected-resource metadata, token audience checks, `MCP-Protocol-Version` handling.
- **OpenAPI.** Derive the document from the same typed routes and `json.Schema` that drive validation, so the description cannot drift from the code. Note that `json.Schema` is a subset of JSON Schema and OpenAPI 3.1+ uses full JSON Schema, which I did not verify in the OpenAPI text I read.
- **llms.txt and Markdown copies.** `site/build.osk` already does this for the Oska site. A service could serve `/llms.txt` and `.md` variants, with `Link` headers for discovery.
- **Authority as API metadata.** Inference: Oska can publish each route's or tool's reachable capabilities (already derived for MCP) in OpenAPI vendor extensions, so an agent client can see that a route is read-only before calling it. No standard defines such an extension, so it would be an Oska convention.

## 5. Security by default, tied to Oska's capabilities and trust

Inference for every item below.

1. **Request data is `Untrusted`.** Path, query, headers, cookies and body are `Untrusted<String>` or `Untrusted<Bytes>` (ADR 0025: everything read from a connection is untrusted). A handler can only put them into HTML through `html.text`, which escapes them, or through an explicit `.assume_trusted()`, which `oskac audit` lists (ADR 0006).
2. **Structural positions become compile errors.** Response header names, the `Location` target, `Set-Cookie` attributes, CSP source lists and any `script` or `style` content should take trusted `String` only. That turns header injection and open redirects into error `E0300`-style failures (ADR 0025 uses `E0300` for untrusted text in a structural position). `std.http` already does this on the client side for request paths and header values (`http.osk` header).
3. **Least authority per route.** Each route holds only the narrowed capabilities it uses, such as a `DirRead` for one directory or a `HostConnect` for one upstream. The authority report lists them per route, and ADR 0016 makes unused authority a release-check error.
4. **SSRF defense is structural.** A handler that fetches a URL gets only a `HostConnect` for fixed hosts. A dynamic host needs a validator and `.assume_trusted()`. `eval/tasks/ssrf-webhook-delivery` and `eval/tasks/ssrf-avatar-fetch` exist for this (not verified: their content).
5. **Classification at the edge.** Logs and responses are sinks with a `Public` ceiling by default (ADR 0029). Session secrets and tokens are `Secret` or `Restricted`, so they cannot reach logs or other users' responses without `Declassify`.
6. **No listener by accident.** Public listening needs its own capability and a reviewed authority baseline entry (ADR 0025 and ADR 0029's rule that widening needs approval).
7. **Defaults that are on:** strict nonce CSP, `nosniff`, HSTS, COOP and CORP, `Referrer-Policy`, `Permissions-Policy`, `__Host-` cookies, CSRF checks on unsafe methods, no CORS, request size and time limits, and a step limit on handlers (ADR 0021 says a step limit stops at the same step on every backend, which suggests a per-request step budget, an inference I did not verify).
8. **Agent-facing safety.** Tool annotations stay derived and are never set by hand (`GLOSSARY.md` lines 85-98). For HTTP MCP, reject tokens not issued for this server and never forward a received token upstream (MCP authorization above).
9. **Hostile-input parsers get the same assurance as TLS:** known-answer and edge-case suites, and live interoperability tests (ADR 0012 names these for TLS). For HTTP, the RFC 9112 smuggling-style ambiguities should be tested. `http.osk` already treats ambiguous framing as an error.

## 6. Recommended phased path

Smallest first step first.

0. **(Smallest) An MCP-over-HTTP, loopback-only service in the existing model, no new protocol.** Add a Streamable HTTP request handler to `std.mcp` that takes a parsed request and returns a response, and run it behind the existing `LoopbackListen`. Reasons: `handle` already exists and is transport-independent, `LoopbackListen` already exists, and the spec's own security warning is about local servers (`Origin` validation and localhost binding). It needs only a small HTTP/1.1 server-side parser and writer. Also serve `/llms.txt`. Tests use `t.net` and recorded requests. It is judged done when a stock MCP client can list and call the notes tools over HTTP, with `Origin` checked.
1. **Server HTTP/1.1 library (`std.http` server half):** strict request parser, response writer, limits and timeouts, and the header-safe types from section 5.2. Reuse the client's strictness rules.
2. **`std.web` layer:** routing with per-route capabilities, cookies and sessions, CSRF, CORS allowlist, default security headers, and nonce-CSP integration with `std.html`. Add the context-aware `html` template literal here, or later.
3. **Public listening and a TLS 1.3 server:** a new listen capability, certificate and key loading with `Secret` keys, ALPN `http/1.1`, and then `h2`. Until then, run behind a TLS-terminating proxy (option B in 4.1).
4. **AI surfaces on the web layer:** OAuth resource-server role for MCP (protected-resource metadata, audience checks), OpenAPI emitted from typed routes, and `llms.txt` plus `.md` variants as a library function lifted from `site/build.osk`.
5. **Assets:** content-hashed static files, SRI and CSP hashes, `.wasm` served as a static asset.
6. **HTTP/2.** Then decide on HTTP/3 and QUIC, a JS parser and minifier, and WASM hosting. These are separate decisions that need an ADR each.

## 7. Open questions

- Whether the classification checker (ADR 0029) is implemented, and how a request's classification would be set. ADR 0029 itself says "not settled".
- Whether a template literal fits the checker's literal mechanism (not read).
- Password hashing: I found no primitive in the std file list, and did not search further.
- The CORS rules were confirmed from MDN only, because the WHATWG Fetch excerpt omitted the CORS section.
- HTTP/2 flood and DoS classes (RFC 9113 section 10) and the ECMAScript and W3C CSS specs were not read, so claims about JS and CSS parsing effort are my own.
- How WASM imports would map to capabilities is open in `docs/design.md` line 571.
- Whether the repo has other files on JS, CSS or web serving that I missed. I searched file names and a few terms, not every crate.

## Sources

1. MCP transports (2025-11-25): https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
2. MCP authorization (2025-11-25): https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization
3. W3C Content Security Policy Level 3: https://www.w3.org/TR/CSP3/
4. OWASP CSRF Prevention Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html
5. OWASP Session Management Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html
6. llms.txt proposal: https://llmstxt.org/
7. OpenAPI Specification v3.2.1: https://spec.openapis.org/oas/latest.html
8. WHATWG Fetch (CORS section not in my excerpt): https://fetch.spec.whatwg.org/#cors-protocol
9. RFC 9113, HTTP/2: https://www.rfc-editor.org/rfc/rfc9113.html
10. RFC 9114, HTTP/3: https://www.rfc-editor.org/rfc/rfc9114.html
11. MDN, CORS: https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS
12. OWASP HTTP Headers Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/HTTP_Headers_Cheat_Sheet.html
13. WebAssembly core spec, introduction: https://webassembly.github.io/spec/core/intro/introduction.html
14. MDN, Trusted Types API: https://developer.mozilla.org/en-US/docs/Web/API/Trusted_Types_API
15. MDN, strict CSP guide: https://developer.mozilla.org/en-US/docs/Web/Security/Practical_implementation_guides/CSP

Oska repo files read (not counted as sources above): `docs/adr/0002`, `0006`, `0011`, `0012`, `0025`, `0029`, `docs/design.md`, `GLOSSARY.md`, `crates/oska-check/src/std/{http,html,tls,mcp,json,sql}.osk`, `crates/oska-check/src/std/rt.osk` (grep only), `examples/notes_server.osk`, `site/build.osk`.

Sources named in the task but not read: RFC 9110, 9112 and 8446 (I relied on Oska's own comments for what they require of the client), the WHATWG HTML and URL specs, the ECMAScript spec, W3C CSS specs, OWASP ASVS.
