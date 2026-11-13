# Research: reviewing oskalang.dev as the site of an AI-first, secure-by-default language

- **Date:** 2026-11-12
- **Method:**
  - I reviewed the live site on 2026-11-12 the way a person and an agent would. I fetched the home page, the guides, the agent setup, the codes, the research and decision sections, `authority.html`, and every machine-readable file. I also read the response headers with `curl -sI`.
  - I crawled the 184 URLs in `sitemap.xml` with a small script and checked every internal `href`, every `#fragment`, every heading sequence, every `<img>`, and the Markdown copy of each page. I then followed every relative link inside those Markdown copies.
  - I read the generator, [`site/build.osk`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk), its stylesheet and script, and the Pages workflow at commit `fec98ec`, which is what the site was built from. Source citations link to that commit.
  - For prior art I fetched only official pages: specifications (llmstxt.org, RFC 9116, CSP Level 3, WCAG 2.2, schema.org, SLSA), vendor documentation (MDN, Google Search Central, GitHub Docs, OpenSSF Scorecard), and the sites of Rust, Go, Zig, Gleam, Roc and Deno.
  - Statements I did not verify are marked as inferences.
- **Question:** How can oskalang.dev be improved as the site of an AI-first, secure-by-default programming language, for people and for agents?
- **Related research:** [AI-driven development](2026-11-ai-driven-development.md) (cited below as *AI-dev*) and [install, update and uninstall](2026-11-install-update-uninstall.md) (cited as *Install*). Where they already make a recommendation, this note refers to it and does not repeat it.

## Summary

The site is strong where most language sites are weak:

- Every page has a Markdown copy, plus `llms.txt`, `llms-full.txt`, `codes.json` and a JSON `latest.json`.
- There is a one-paste agent setup prompt that refuses unverified installs.
- Every page carries a strict CSP, and the CSS loaded from the CDN is pinned with Subresource Integrity.
- No internal HTML link is broken. Each page has one `<h1>` and heading levels never skip.
- The generator publishes its own authority report.

The problems are mostly in what surrounds that core:

1. **Agents meet dead links that people never see.** The generator drops a link to any repository file it does not publish. In HTML it keeps only the link text. In the Markdown copies, which agents read, it leaves the link as written, so it resolves to a 404 on oskalang.dev. That is 35 distinct dead targets across 8 Markdown pages. The two guide example pages are never published at all.
2. **There is nowhere to report a vulnerability.** The site and the repository have no security policy and no `security.txt`, and GitHub private vulnerability reporting is off. For a language whose pitch is security, this is the most visible missing trust signal.
3. **The home page overclaims.** It promises SQL, process and logging APIs that cannot accept untrusted input. But `std.sql` and `std.proc` only check and do not run, and no logging module exists. The agent guide states the limits honestly; the front page does not.
4. **The first-run path is buried.** The home page is the repository README. It has no human install command, links to itself, ends with contributor build steps, and its navigation leads with compiler internals (AST, Checker, Runtime) rather than Install or Quickstart.
5. **Agents get one undifferentiated corpus.** `llms-full.txt` is 565 KB. 21% of it is research proposals and 20% is code pages that repeat `checker.md`. Nothing tells an agent what it can skip, though llms.txt has a convention for exactly that.

**Recommendations.** The top five are:

- fix link handling and publish the examples;
- publish a security policy, enable private reporting, and serve `security.txt`;
- correct the home page claims and add a "what works today" table;
- rebuild the first-run path: a home page with both install routes, and navigation that leads with tasks;
- tier `llms.txt` and `llms-full.txt`.

The full ranked list is at the end. Most items are effort S and fit the generator without new authority.

## Part 1: findings on the live site

### 1. What an agent gets, and where it breaks

**What works:**

- **`llms.txt`** follows the llmstxt.org shape: an H1, a blockquote, a details paragraph, then H2 file lists ([live](https://oskalang.dev/llms.txt); [`build.osk:431-456`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L431-L456)).
- **Every page has a Markdown copy,** found through `<link rel="alternate" type="text/markdown">` ([`build.osk:696-700`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L696-L700)). This matches the llmstxt.org proposal ([llmstxt.org](https://llmstxt.org/)), and the crawl found a copy for every sitemap URL.
- **`robots.txt`** welcomes AI crawlers by name ([live](https://oskalang.dev/robots.txt); [`build.osk:393-401`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L393-L401)).

**Finding 1a: dead links in the Markdown copies.**

- **Cause:** a relative link to a repository file the site does not publish is resolved by `safe_href`. When `published_url` finds no page, it returns `None` ([`build.osk:1383-1400`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1383-L1400), [`1444-1457`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1444-L1457)).
  - The HTML renderer then drops the anchor and keeps its text.
  - The Markdown retargeter leaves the destination unchanged: "a link that goes nowhere" is left alone ([`build.osk:1090-1171`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1090-L1171)).
- **Effect:** a crawl of the Markdown copies found 35 distinct 404 targets. Examples:
  - `https://oskalang.dev/LICENSE` and `https://oskalang.dev/examples/shopping`, from [`/index.md`](https://oskalang.dev/index.md) (README lines [54](https://github.com/aparsons/oska/blob/fec98eccf2d3/README.md?plain=1#L54) and [85](https://github.com/aparsons/oska/blob/fec98eccf2d3/README.md?plain=1#L85));
  - `https://oskalang.dev/guide/examples/capabilities.md`, from [`/guide/index.md`](https://oskalang.dev/guide/index.md), [`/guide/quickstart.md`](https://oskalang.dev/guide/quickstart.md) and [`/guide/language-tour.md`](https://oskalang.dev/guide/language-tour.md);
  - `https://oskalang.dev/guide/examples/hello.md`, from `/guide/index.md`;
  - 31 more `scripts/`, `crates/`, `site/` and `.github/` paths, from the two published research notes.
- The same text goes into `llms-full.txt` ([`build.osk:475-485`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L475-L485)).
- The HTML crawl found no broken link only because the anchors had already been removed.
- **This note avoids adding to the problem:** it links source files as GitHub permalinks.

**Finding 1b: the guide examples are never published.**

- `gather` reads `docs/*.md` and one level of subdirectories, so `docs/guide/examples/hello.md` and `capabilities.md` are skipped ([`build.osk:190-211`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L190-L211)).
- The guide index's "Examples" section therefore renders as two bare words, "hello" and "capabilities", with no links ([live](https://oskalang.dev/guide/); [`docs/guide/index.md:29-30`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/index.md?plain=1#L29-L30)).
- Both pages return 404.

**Finding 1c: `llms-full.txt` has no tiers.**

- It is 564,914 bytes and 85,511 words, about 169 KB gzipped.
- By section:
  - research: 20.8%
  - code pages: 20.3%
  - guides: 19.1%
  - `checker.md`: 13.3%, and it already holds the whole code table and examples that the code pages repeat
  - design: 7.6%
  - ADRs: 5.6%
  - (my measurement of the live file)
- The order is README, glossary, internals, ADRs, guides, research, codes, then agent setup ([`build.osk:113-140`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L113-L140)). An agent that truncates a long file keeps the internals and loses the setup prompt.
- The llms.txt proposal reserves a section titled "Optional" for "secondary information", so an agent can skip it when it needs less context ([llmstxt.org](https://llmstxt.org/)). The site does not use it.

**Finding 1d: two small data defects.**

- **Truncated summary.** The code index summarises each code by its first sentence, found by splitting on `". "` ([`build.osk:1804-1810`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1804-L1810)). E0103's message starts "A bare `{ ... }` block", so its summary reads only "A bare `{ ." ([live](https://oskalang.dev/codes/)).
- **No URLs in `codes.json`.** It is a bare array with no `url` per entry and no version ([live](https://oskalang.dev/codes.json); [`build.osk:1816-1835`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1816-L1835)). *AI-dev* A3 already proposes a `url` in diagnostics.

**Finding 1e: no API reference.** An agent can go from zero to a checked "hello" using only the site: `llms.txt`, then [`agent-setup.md`](https://oskalang.dev/agent-setup.md), then [quickstart](https://oskalang.dev/guide/quickstart.md). It then has no list of what `List`, `String` or `std.*` provide. This is *AI-dev* D2 (Part 1 §5); the site side is proposal A5 below.

### 2. The first-run path for people

- **The home page is the README** ([`build.osk:183-184`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L183-L184)). On the [live page](https://oskalang.dev/):
  - "Install, docs and guides: oskalang.dev" links the site to itself ([README:5](https://github.com/aparsons/oska/blob/fec98eccf2d3/README.md?plain=1#L5)).
  - The only install path is the agent prompt. There is no human one-line installer command and no link to [Install](https://oskalang.dev/guide/install.html).
  - The page ends with "Repository layout" and "Building" (`cargo build`) ([README:73-80](https://github.com/aparsons/oska/blob/fec98eccf2d3/README.md?plain=1#L73-L80)).
- **The navigation leads with internals:** Oska, AST, Checker, Design, Runtime, Glossary, Agent setup, Codes, Decisions, Guides, Research. It is built from the docs tree, not from tasks ([`build.osk:719-737`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L719-L737)). Install, Quickstart and Language tour are one click deeper.
- **The install page puts "Build from source" (Rust) before the one-line installers** ([`docs/guide/install.md` headings at lines 5 and 27](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/install.md?plain=1#L5-L27)).
- **The guide index says the guides describe "v0.1.0 or later"**, while the release is v0.3.0 ([`docs/guide/index.md:3`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/index.md?plain=1#L3)).
- **The 404 page is GitHub's default** ("Page not found · GitHub Pages"), with no search box or links back. Every unknown path returns it, including `/favicon.ico`, which browsers request on every visit (curl, 2026-11-12).

### 3. Claims a security-minded reader will check

- **The home page overstates the standard library.** It says: "The standard library's SQL, HTML, process, and logging APIs cannot accept untrusted input…" ([README:15](https://github.com/aparsons/oska/blob/fec98eccf2d3/README.md?plain=1#L15)). The facts:
  - The modules that exist are `std.sql`, `std.proc` and `std.html`. There is no logging module ([`docs/checker.md:86-90`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/checker.md?plain=1#L86-L90)).
  - "`std.sql` and `std.proc` have none [no runtime] yet" ([`checker.md:92`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/checker.md?plain=1#L92)).
  - The agent guide is honest about this: "`Net`, `std.sql`, and `std.proc` check but do not run" ([`agent-guide.md:128`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/agent-guide.md?plain=1#L128)).
  - The front page and `llms.txt`'s first lines are where both readers form their model of the language.
- **There is no vulnerability-reporting channel anywhere:**
  - `/.well-known/security.txt` and `/security.txt` return 404.
  - The repository has no `SECURITY.md`. GitHub's community profile lists only a licence and a README (health 42%).
  - Private vulnerability reporting is `enabled: false`, and secret scanning and Dependabot security updates are disabled.
  - (`gh api`, 2026-11-12)
- **The signing key has only one channel, and its MIME types are misleading:**
  - The site serves the release key at `/oska-release.pub` and the installers pin it at build time (*Install* Part 1 §3). Its fingerprint, `SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o`, appears nowhere on the site, so a reader cannot cross-check it against a second channel.
  - GitHub Pages types by extension, so the key is served as `application/x-mspublisher` and the SSH signature as `application/pgp-signature` (curl, 2026-11-12). This is cosmetic, but a browser downloads the key instead of showing it.
- **No release notes or build provenance:**
  - The GitHub release body is one line, "Download from …/releases/v0.3.0/". That page 404s today; the fix is in PR #148.
  - "Upgrading to v0.3.0" lives inside the install guide ([`install.md:105`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/install.md?plain=1#L105)).
  - No workflow produces build provenance or attestations: a grep of `.github/workflows` for `attest` or `provenance` found nothing.
- **What is already a strong trust signal:**
  - the published [authority report](https://oskalang.dev/authority.html) of the generator;
  - the installers' signature, tag-line and checksum checks ([install guide](https://oskalang.dev/guide/install.html));
  - docs whose examples are checked by `cargo test` ([`guide/index.md:3`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/index.md?plain=1#L3)).

### 4. HTTP headers and what GitHub Pages allows

**What the server sends.** `curl -sI https://oskalang.dev/` returns only `content-type`, `cache-control: max-age=600`, `access-control-allow-origin: *`, `etag`, `last-modified` and CDN headers. It sends no `Strict-Transport-Security`, `Content-Security-Policy`, `X-Content-Type-Options`, `Referrer-Policy` or `X-Frame-Options`. GitHub's Pages HTTPS documentation offers HTTPS enforcement and says nothing about custom headers ([GitHub Docs: HTTPS for Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/securing-your-github-pages-site-with-https)). *Inference:* Pages cannot set them.

**What this means for each control:**

- **HSTS is not needed.** ".dev is included on the HSTS preload list, making HTTPS required on all connections" ([Google Registry: .dev](https://www.registry.google/domains/dev/)). `http://` and `www.` both 301 to `https://oskalang.dev/`.
- **The CSP is delivered as a `<meta>` element and is good:**
  - `default-src 'none'`
  - `script-src 'self'`
  - `connect-src 'self'`
  - style and font sources limited to two version-pinned CDN paths
  - `base-uri 'none'`
  - `form-action 'none'`
  - ([`build.osk:657-659`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L657-L659))
- **Some CSP directives cannot work this way.** CSP Level 3 says `report-uri`, `frame-ancestors` and `sandbox` are not supported in a `<meta>` element ([CSP3 §3.3](https://www.w3.org/TR/CSP3/#meta-element)). MDN says a meta-delivered policy "does not support all CSP features" ([MDN: CSP](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CSP)). So framing (clickjacking) protection and violation reporting are impossible on Pages. For a static docs site with no forms or state, I judge the clickjacking risk low (*inference*).
- **`Referrer-Policy` could be set with `<meta name="referrer">`.** Browsers already default to `strict-origin-when-cross-origin`, so the gain is small.
- **SRI covers the CSS but not the fonts it loads.**
  - SRI applies to `<script>` and to `<link>` with `rel` `stylesheet`, `preload` or `modulepreload` ([MDN: SRI](https://developer.mozilla.org/en-US/docs/Web/Security/Subresource_Integrity)).
  - The three Fontsource stylesheets carry `integrity` and `crossorigin` ([`build.osk:666-677`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L666-L677)).
  - The `.woff2` files those stylesheets pull in through `url()` are outside SRI's documented scope. They are constrained only by the CSP's version-pinned `font-src` path (*inference* from MDN's scope).
- **The fonts dominate page weight, from a third-party origin.** They are 69 KB (23.7 + 24.4 + 21.2 KB). The home page's own HTML, CSS and JS total about 25 KB (14.6 + 4.6 + 5.5 KB).

### 5. Discoverability

- **No meta description or Open Graph tags:**
  - None of the 184 pages has `<meta name="description">` or `og:` tags (crawl), though the generator already computes a 200-character summary for every page, for `llms.txt` ([`build.osk:578-596`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L578-L596)).
  - Google uses the meta description "if it might give users a more accurate description" than the content ([Google: snippets](https://developers.google.com/search/docs/appearance/snippet)).
  - The home `<title>` is just "Oska".
- **No `<lastmod>` in the sitemap** ([`build.osk:405-412`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L405-L412)). Google uses `<lastmod>` only "if it's consistently and verifiably accurate", and ignores `priority` and `changefreq` ([Google: build a sitemap](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap)).
- **The JSON-LD is valid but earns no rich result:**
  - The home page carries `SoftwareSourceCode` with `targetProduct` → `SoftwareApplication` ([`build.osk:711-715`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L711-L715)). Each property is valid schema.org: `version` comes from CreativeWork, `targetProduct` expects SoftwareApplication, and `programmingLanguage` accepts Text ([schema.org/SoftwareSourceCode](https://schema.org/SoftwareSourceCode)).
  - Google's software-app rich result requires `offers.price` and `aggregateRating` or `review` ([Google: software app](https://developers.google.com/search/docs/appearance/structured-data/software-app)). The site cannot honestly supply a rating, so the markup serves agents and other parsers, not Google's display.

### 6. Accessibility and performance

**What already passes (crawl and HTML):**

- `lang="en"`.
- One `<h1>` per page and no skipped heading levels.
- Landmarks: `<header>`, `<nav aria-label="Site">`, `<main>`, `<footer>`, and an `<aside aria-label="Version">`.
- `aria-current="page"` in the navigation.
- No images (so no missing `alt`).
- Light and dark colour schemes.
- Text contrast passes AA. Muted on surface is 5.5:1; the link accent is 6.7:1 on white and 8.2:1 in dark mode (computed from [`style.css:7-27`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/style.css#L7-L27)).

**Gaps:**

- **The search box's border is 1.31:1** (`#dde1e6` on white; [`style.css:12,85`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/style.css#L85)).
  - The box has no visible label, only a placeholder and `aria-label` ([`search.js:114-115`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/search.js#L114-L115)).
  - WCAG 2.2 SC 1.4.11 requires 3:1 where "the border is required to identify the presence of the input" ([W3C: Understanding 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)).
- **No `:focus-visible` style and no skip link.**
  - The stylesheet defines no focus style, so the browser default applies (`grep focus style.css`: none).
  - There is no skip link before the 11-item navigation.
  - The landmarks already satisfy SC 2.4.1 through technique ARIA11 ([W3C: Understanding 2.4.1](https://www.w3.org/WAI/WCAG22/Understanding/bypass-blocks.html)). A skip link (G1) still helps sighted keyboard users.
- **Weight is modest:**
  - The largest page is `checker.html` at 100 KB.
  - `search.json` (226 KB, 64 KB gzipped) loads only on first use of the search box ([`search.js:125-130`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/search.js#L125-L130)).

## Part 2: prior art

| Site | What it offers that oskalang.dev lacks | Source |
| --- | --- | --- |
| Rust | A security policy page: report to security@rust-lang.org, acknowledged within 24 hours, a detailed reply within 48 hours, a five-step disclosure process, and announcements via the mailing list and blog | [rust-lang.org/policies/security](https://www.rust-lang.org/policies/security) |
| Rust | Versioned standard-library docs at a fixed path (`doc.rust-lang.org/1.80.0/std/` shows "std 1.80.0") | [doc.rust-lang.org/1.80.0/std](https://doc.rust-lang.org/1.80.0/std/) |
| Go | A security policy with PUBLIC, PRIVATE and URGENT tracks, a 7-day acknowledgement, a fix or disclosure within 90 days, and "Please do not send LLM-generated reports without proper curation" | [go.dev/doc/security/policy](https://go.dev/doc/security/policy) |
| Go | A release history: each major version links to its release notes, and each minor version separates "security fixes" from "bug fixes" | [go.dev/doc/devel/release](https://go.dev/doc/devel/release) |
| Zig | The download page prints the minisign public key and a `.minisig` per file, and offers `index.json`. Each release links its notes and its version's language reference and std docs | [ziglang.org/download](https://ziglang.org/download/) |
| Gleam | "Try Gleam" runs an in-browser language tour (tour.gleam.run), with a playground, News, and Install in the top navigation | [gleam.run](https://gleam.run/) |
| Roc | "You can run the full compiler in the browser without a backend server", with a runnable example on the home page and a tutorial as the main call to action | [roc-lang.org](https://www.roc-lang.org/) |
| Deno | A permissions page that states the model's limits: "Deno sets no limits on the execution of code at the same privilege level"; `--allow-run` and `--allow-ffi` amount to full access | [docs.deno.com: security](https://docs.deno.com/runtime/fundamentals/security/) |

**Standards and checkers:**

- **security.txt (RFC 9116).**
  - `Contact` "MUST always be present" (§2.5.3).
  - `Expires` "MUST always be present and MUST NOT appear more than once" (§2.5.5).
  - The file lives under `/.well-known/` (§3).
  - An OpenPGP cleartext signature is RECOMMENDED, not required (§2.3).
  - ([RFC 9116](https://www.rfc-editor.org/rfc/rfc9116.html))
- **OpenSSF Scorecard:**
  - *Security-Policy* looks for `SECURITY.md` and scores contact links, free text, and disclosure wording.
  - *Signed-Releases* gives 8/10 when every release has a signature asset (`.sig` counts, so Oska's `SHA256SUMS.sig` should qualify) and 10/10 when it also has `.intoto.jsonl` provenance.
  - *Pinned-Dependencies* wants actions pinned by hash.
  - ([Scorecard checks](https://github.com/ossf/scorecard/blob/main/docs/checks.md))
  - The workflows pin the Rust toolchain action by SHA, but `actions/checkout@v4` and the artifact actions only by tag ([`pages.yml:28`](https://github.com/aparsons/oska/blob/fec98eccf2d3/.github/workflows/pages.yml#L28), [`release.yml:88,118`](https://github.com/aparsons/oska/blob/fec98eccf2d3/.github/workflows/release.yml#L88)).
- **SLSA build levels.**
  - L1: "Provenance exists".
  - L2: a hosted build platform, with provenance "tied to that infrastructure through a digital signature".
  - L3: hardened builds whose signing secrets are not reachable from build steps.
  - ([SLSA v1.0 levels](https://slsa.dev/spec/v1.0/levels); the page notes v1.2 is current.)
- **GitHub artifact attestations** need `id-token: write` and `attestations: write`, and are verified with `gh attestation verify <file> -R <owner>/<repo>` ([GitHub Docs: artifact attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds)).
- **GitHub private vulnerability reporting** gives "a secure, structured way to disclose vulnerabilities directly in your repository". An admin enables it in Settings → Advanced Security ([GitHub Docs](https://docs.github.com/en/code-security/security-advisories/working-with-repository-security-advisories/configuring-private-vulnerability-reporting-for-a-repository)).
- **A custom 404 page** is a file named `404.html` at the root of the publishing source ([GitHub Docs: custom 404](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-custom-404-page-for-your-github-pages-site)).

## Part 3: proposals

**How each proposal fits.** The generator is an Oska program with no external crates. It reads only the paths `main` narrows to ([`build.osk:56-72`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L56-L72)), reads text only (`read_to_string`), and has no clock. Anything binary, or anything dated, belongs in `pages.yml` or `stage-site.sh`, the way `authority.txt` and the releases are added today ([`pages.yml:31-49`](https://github.com/aparsons/oska/blob/fec98eccf2d3/.github/workflows/pages.yml#L31-L49)). A new input to the generator is a new attenuation in `main`, and it shows up in the published authority report. That is a feature: say so in the PR.

Effort is S (hours), M (days) or L (weeks).

### A. Agent consumption

**A1. Make every link go somewhere, and fail the build when one does not.**

- **What:**
  - In `safe_href`, when a relative link resolves to a repository path that is not published, link to `https://github.com/aparsons/oska/blob/<ref>/<path>` instead of dropping it. `<ref>` is `main`, which matches "the docs track `main`".
  - Use the same URL in the Markdown copy.
  - Recurse one level deeper in `gather`, so that `docs/guide/examples/` is published.
  - Have the build fail, or at least print, for any link that resolves outside the repository.
- **Why:** this fixes the 35 dead targets in the agent-facing Markdown and the two unlinked examples (Findings 1a and 1b). People and agents then see the same links.
- **Fit:**
  - `published_url` gets a fallback branch ([`build.osk:1444-1457`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1444-L1457)).
  - Recursing into `examples` needs no new authority, because `docs` is already readable.
  - Extend `pages.yml`'s smoke step, or a test in `site/tests/`, to run the Markdown-link crawl used for this note.
- **Effort:** S. **Risk:** low. Links into `main` can drift from a released version until D1 exists.

**A2. Tier `llms.txt`: a "Start here" list first, and an `## Optional` section.**

- **What:**
  - Open with a short ordered path: agent setup, quickstart, language tour, capabilities, untrusted data, agent guide, codes.
  - Move research and the internals that most programs do not need (AST export, runtime internals) under `## Optional`, the section llmstxt.org reserves for skippable content ([llmstxt.org](https://llmstxt.org/)).
- **Why:** today Decisions come before Guides, and research is listed as if it were documentation ([live](https://oskalang.dev/llms.txt)).
- **Fit:** `llms_txt` already groups pages by section ([`build.osk:431-456`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L431-L456)), so this is a different grouping.
- **Effort:** S. **Risk:** low.

**A3. Slim `llms-full.txt` and order it for truncation.**

- **What:**
  - Order it: agent setup, guides, glossary, design, checker, ADRs.
  - Leave out the per-code pages, which `checker.md` and `codes.json` already hold.
  - Move research to a separate `llms-research.txt`, or out entirely.
- **Why:** about 41% of the 565 KB is research or duplicated codes (Finding 1c). The order should not drop the setup prompt when an agent truncates.
- **Fit:** filter and sort in `llms_full` ([`build.osk:475-485`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L475-L485)).
- **Effort:** S. **Risk:** low. Removing pages from the file is visible to anyone who greps it; say so in `llms.txt`.

**A4. Fix the code summaries, and add `url` to `codes.json`.**

- **What:**
  - Split the first sentence at `". "` only outside code spans: reuse `plain_text`, or take the summary from the table's message without code.
  - Add `"url": "https://oskalang.dev/codes/<code>"` to each entry, together with *AI-dev* A3's `url` in diagnostics, so that `oskac explain --format=json` stays field-for-field the same ([`build.osk:1825-1835`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L1825-L1835)).
- **Effort:** S. **Risk:** low.

**A5. Publish the prelude and `std` API reference on the site.**

- **What:** the site half of *AI-dev* D2: `/api/` pages, `api.json`, and entries in `llms.txt` and `llms-full.txt`.
- **Fit:**
  - Either the generator reads `crates/oska-check/src/prelude.osk` and `std/*.osk` (a new `restrict_to` in `main`, visible in the authority report), or the workflow writes `oskac doc --format=json` output for it to read, as with `authority.txt`.
  - The second keeps the generator from parsing Oska itself.
- **Effort:** M. **Risk:** medium. The reference must be generated and tested, never hand-written.

**A6. Add a "What works today" table as a page and as data.**

- **What:** one table: feature × checks / runs on the interpreter / runs native / platforms. Include `std.sql`, `std.proc`, `Net`, contracts, packages and native targets, and link it from `llms.txt` and the home page.
- **Why:**
  - Finding 3: the limits are true but scattered ([`agent-guide.md:128`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/agent-guide.md?plain=1#L128), [`checker.md:92`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/checker.md?plain=1#L92)).
  - An agent that believes `std.sql` runs will write code that checks and then fails.
- **Fit:** a Markdown table in `docs/guide/`, held to the toolchain by the existing docs tests, the way the CLI reference is held to `--help`.
- **Effort:** S. **Risk:** low.

### B. The first-run path

**B1. Separate the landing page from the README, or reorder the README.**

- **What:**
  - Make the first screen hold three things: one sentence on what Oska is, the human one-liner (`curl -fsSL https://oskalang.dev/install.sh | sh`, with the PowerShell form), and the agent prompt.
  - Then show a 5-line program with its authority report, then "Quickstart →".
  - Remove the self-link.
  - Move "Repository layout" and "Building" below the fold or to a contributing page.
- **Why:** Gleam leads with "Try Gleam" and Install, and Roc with a runnable example and a tutorial ([gleam.run](https://gleam.run/); [roc-lang.org](https://www.roc-lang.org/)). Today oskalang.dev's only install path on the home page is the agent prompt (Part 1 §2).
- **Fit:**
  - Option 1: a `docs/home.md` that `gather` maps to `index.html` in place of the README. This needs one more `restrict_to_file` or a rule in `gather`.
  - Option 2: keep README-as-home and reorder the README, which is also GitHub's front page.
- **Effort:** S–M. **Risk:** low.

**B2. Navigation by task.**

- **What:**
  - Use Install, Quickstart, Tour, Guides, Agent setup, Codes, Security, and a Reference menu or page.
  - Move Design, Checker, Runtime, AST, Glossary and Decisions under Reference, and Research under Reference or the footer.
- **Fit:** `navigation` builds the list ([`build.osk:719-737`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L719-L737)). A fixed, hand-ordered list is simpler than deriving it from the tree.
- **Effort:** S. **Risk:** low.

**B3. Fix the install page and the guide index.**

- **What:**
  - Put "One-line installers" first and "Build from source" last.
  - Show the release key's fingerprint, with a three-line manual verification (`ssh-keygen -Y verify …`, then `sha256sum -c`).
  - Fix "v0.1.0 or later" ([`guide/index.md:3`](https://github.com/aparsons/oska/blob/fec98eccf2d3/docs/guide/index.md?plain=1#L3)).
  - *Install* F2 and F3 (receipt, `--uninstall`) change the Update and Uninstall sections. Coordinate with them.
- **Effort:** S. **Risk:** low.

**B4. A browser playground: defer it.**

- **What:** Gleam's tour and Roc's in-browser compiler show the value ([gleam.run](https://gleam.run/); [roc-lang.org](https://www.roc-lang.org/)). For Oska it would mean compiling the checker and interpreter to WebAssembly and loading it under the CSP (`script-src 'self'`, plus `'wasm-unsafe-eval'`).
- **Why defer:** it adds a large JavaScript and Wasm surface to a site whose selling point is a tight CSP, and agents, the primary audience, run `oskac` locally.
- **Instead:** show each quickstart step's exact output, which the guides already do and test.
- **Effort:** L. **Risk:** medium.

### C. Security posture and trust signals

**C1. A security policy, private reporting, and a `/security` page.**

- **What:**
  - Add `SECURITY.md` and publish it as `/security.html` and `.md`. The generator would need it as a new input, or it could live at `docs/security.md`.
  - The policy should cover:
    - how to report: GitHub private vulnerability reporting, which must be enabled ([GitHub Docs](https://docs.github.com/en/code-security/security-advisories/working-with-repository-security-advisories/configuring-private-vulnerability-reporting-for-a-repository));
    - an acknowledgement target;
    - what counts as a vulnerability: the checker accepting a program that reaches authority it was not given, a taint bypass, an installer verification bypass, and site injection;
    - which versions are supported (pre-alpha: latest only);
    - where advisories appear.
  - Borrow Go's tracks, and its line on uncurated LLM reports ([go.dev](https://go.dev/doc/security/policy)).
- **Why:** this is Scorecard Security-Policy ([checks](https://github.com/ossf/scorecard/blob/main/docs/checks.md)). Rust and Go both put this on their main sites (Part 2).
- **Effort:** S. **Risk:** low. It commits the maintainer to response times; choose honest ones.

**C2. `/.well-known/security.txt`.**

- **What:** `Contact:` (the private-reporting URL), `Expires:`, `Policy: https://oskalang.dev/security.html`, `Canonical:`, `Preferred-Languages: en` ([RFC 9116](https://www.rfc-editor.org/rfc/rfc9116.html)).
- **Fit:**
  - `Expires` needs a date and the generator has no clock.
  - Write the file in `pages.yml` or `stage-site.sh` with `date -d '+1 year'`, which the weekly or per-push deploys refresh.
  - The generator would need a `.well-known` directory capability. Putting the file in the workflow keeps the generator's authority unchanged.
  - An OpenPGP signature is optional, and Oska signs with SSH keys, so skip it.
- **Effort:** S. **Risk:** low. A stale `Expires` is a defect, so have the smoke job check that it is in the future.

**C3. A threat-model page: what Oska guarantees, and what it does not.**

- **What:** one page, in the style of Deno's limits section ([docs.deno.com](https://docs.deno.com/runtime/fundamentals/security/)), stating:
  - **The guarantees:** no ambient authority, the taint rules, and confinement.
  - **Where they stop:**
    - `oska-sys` is the only `unsafe` (ADR 0009);
    - raw system calls are only in `std.rt` (ADR 0013);
    - what the interpreter and the native backend each enforce;
    - `assume_trusted` and `expose` as audit sites.
  - **The supply chain:**
    - one signing key;
    - the key and the installer come from the same origin;
    - `latest.json` is unsigned (*Install* G2, F4).
  - **What the site itself trusts:** GitHub Pages, jsDelivr fonts, and no framing protection (Part 1 §4).
- **Why:** a security-minded evaluator looks for this first, and the authority report is only meaningful when the reader knows its boundary.
- **Effort:** M (writing). **Risk:** low.

**C4. A second channel for the release key's fingerprint.**

- **What:** print the fingerprint in the install guide, in `SECURITY.md`, and in every GitHub release body, the way Zig prints its minisign key on the download page ([ziglang.org/download](https://ziglang.org/download/)). Add it to `latest.json` as `signing_key_fingerprint`.
- **Why:** today a compromise of the site could swap the installer and the key together. A fingerprint published on GitHub as well lets a careful person or agent catch that. *Inference:* it does not help the pinned one-liner, which trusts the site by design.
- **Effort:** S. **Risk:** low. Rotation (*Install* F12) must update every channel.

**C5. Build provenance for release binaries.**

- **What:**
  - Add GitHub artifact attestations to `release.yml` (`id-token: write`, `attestations: write`) ([GitHub Docs](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds)).
  - Publish the bundle beside `SHA256SUMS` on `/releases/<tag>/`, and document `gh attestation verify`.
- **Why:**
  - It gives SLSA Build L2-style signed provenance from a hosted builder ([SLSA levels](https://slsa.dev/spec/v1.0/levels)).
  - It moves Scorecard Signed-Releases from 8 to 10 when an `.intoto.jsonl` asset is present ([checks](https://github.com/ossf/scorecard/blob/main/docs/checks.md)).
  - It complements, not replaces, the SSH signature the installer checks.
- **Fit:** this touches the release workflows, so follow `docs/agents/release.md`. `verify.sh` refuses unlisted files in a release directory ("no unlisted file is present", *Install* Part 1 §2), so the bundle must be listed or exempted.
- **Effort:** M. **Risk:** medium (release pipeline).

**C6. Self-host the fonts, or drop them.**

- **What:**
  - Option 1: commit the three `.woff2` files and copy them into `_site/fonts/` in `pages.yml`. The generator reads text only.
  - Option 2: use the system font stack the CSS already falls back to ([`style.css`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/style.css)).
  - Either way, the CSP shrinks to `'self'` for styles and fonts.
- **Why:**
  - It removes the only third-party origin.
  - SRI does not cover the font files (Part 1 §4).
  - It removes 69 of roughly 95 KB of first-load weight.
  - It stops sending every visitor's IP address to jsDelivr.
- **Effort:** S. **Risk:** low.

**C7. Headers: document the limits, and do not add a proxy for them.**

- **What:**
  - Add `<meta name="referrer" content="strict-origin-when-cross-origin">` if you want it explicit.
  - In C3, record that `frame-ancestors` and reporting cannot be set on Pages ([CSP3 §3.3](https://www.w3.org/TR/CSP3/#meta-element)) and that HSTS comes from the .dev preload ([registry.google](https://www.registry.google/domains/dev/)).
- **Why not a proxy:** putting a CDN in front only for headers adds a party to the trust chain for little gain on a static site (*inference*).
- **Effort:** S. **Risk:** none.

**C8. Repository hygiene that Scorecard and reviewers see.**

- **What:** enable secret scanning and push protection, enable Dependabot security updates, and pin `actions/*` by SHA (Part 2).
- **Why here:** these are repository settings, but the site links the repository on every page, and evaluators check both.
- **Effort:** S. **Risk:** low.

### D. Documentation structure and versioning

**D1. Versioned docs for each release.**

- **What:**
  - At release staging, build the docs from the tag into `/v<version>/`, and keep `/` as `main`.
  - The banner already says the docs track `main` ([`build.osk:703-705`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L703-L705)); add "docs for v0.3.0 →".
  - Point `llms.txt` at both.
- **Why:** Rust serves `doc.rust-lang.org/1.80.0/std/`, and Zig links each release to its own reference ([doc.rust-lang.org](https://doc.rust-lang.org/1.80.0/std/); [ziglang.org/download](https://ziglang.org/download/)). An agent pinned to v0.3.0 by the agent-setup CI step should read v0.3.0 docs.
- **Fit:**
  - In `pages.yml`, for each stable tag: `git worktree add` the tag, run that tag's `oskac run site/build.osk` there, and move its `_site` to `_site/v<version>/`.
  - Older tags' generators write root-relative links (`/guide/…`), so they need a base-path option first. That is a change to `origin()` and `url_of` ([`build.osk:369-384`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L369-L384)), from a release onwards.
- **Effort:** M. **Risk:** medium (build time, disk, and old generators).

**D2. Release notes and a releases index.**

- **What:**
  - Add `/releases/`: newest first, with the date and notes, and security fixes listed separately in Go's style ([go.dev release history](https://go.dev/doc/devel/release)).
  - Move "Upgrading to v0.x" there from the install guide.
  - Make the GitHub release body carry the notes and the key fingerprint (C4).
- **Fit:** PR #148 already adds the `/releases/vX.Y.Z/` pages. A `docs/releases.md` or `CHANGELOG.md` is one more doc for the generator.
- **Effort:** S–M. **Risk:** low.

**D3. Mark research as non-normative.**

- **What:** a banner on `/research/` pages: "Proposals and background, not documented behaviour. See the guides for what Oska does." Combine it with A2's `## Optional`.
- **Why:** these notes say "should" about features that do not exist, and they are 21% of `llms-full.txt`.
- **Effort:** S. **Risk:** none.

### E. Discoverability and SEO

**E1. A meta description and Open Graph tags on every page.**

- **What:** from `summary_of`, which already exists ([`build.osk:578-596`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L578-L596)), add `<meta name="description">`, `og:title`, `og:description`, `og:url` and `og:type`. Make the home `<title>` "Oska: an AI-first, secure-by-default programming language".
- **Why:** Google may use the description for snippets ([Google: snippets](https://developers.google.com/search/docs/appearance/snippet)), and link unfurls in chat and issues use OG tags.
- **Fit:** add it to `about` in `layout`. The text goes through `html.text`.
- **Effort:** S. **Risk:** low.

**E2. `<lastmod>` from git.**

- **What:** in `pages.yml`, write `site/lastmod.txt` (path, then the date of the last commit to touch the source) before the build. The generator reads it, like `authority.txt`, and adds `<lastmod>`.
- **Why:** Google uses `<lastmod>` only if it is accurate ([Google: sitemaps](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap)), and git dates are accurate.
- **Effort:** S–M. **Risk:** low. It is a new generator input, which appears in the authority report.

**E3. A favicon and a custom 404.**

- **What:**
  - Add an SVG favicon. It is text, so the generator can copy it, and `img-src 'self'` allows it.
  - Add a `404.html` built by the generator, with search, Install, Guides and `llms.txt` links ([GitHub Docs: custom 404](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-custom-404-page-for-your-github-pages-site)).
- **Why:** today every visit triggers a `/favicon.ico` 404, and a mistyped URL lands on GitHub's branded page.
- **Effort:** S. **Risk:** none.

**E4. JSON-LD: keep it as it is.**

- **What:** keep `SoftwareSourceCode`. Do not add `offers` and a rating to chase Google's software-app rich result ([Google](https://developers.google.com/search/docs/appearance/structured-data/software-app)): a free pre-alpha compiler has no honest rating.
- **Optional:** add `softwareHelp` (the guides) and `downloadUrl` (`latest.json`'s release page), both valid on `SoftwareApplication`.
- **Effort:** S. **Risk:** none.

### F. Accessibility and performance

**F1. Search box contrast and a visible focus style.**

- **What:** give `.search input` a border of at least 3:1 (for example `--muted`, `#5b6470`, at 6.0:1 on white), and add a `:focus-visible` outline in `--accent`.
- **Why:** WCAG 2.2 SC 1.4.11 ([W3C](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)); Part 1 §6.
- **Fit:** `style.css` only.
- **Effort:** S. **Risk:** none.

**F2. A skip link.**

- **What:** add `<a class="skip" href="#content">Skip to content</a>`, shown on focus, and `id="content"` on `<main>`.
- **Why:** it is technique G1 for SC 2.4.1 ([W3C](https://www.w3.org/WAI/WCAG22/Understanding/bypass-blocks.html)), in front of the 11-item navigation. The landmarks already meet the criterion, so this is for sighted keyboard users.
- **Fit:** `layout` ([`build.osk:680-691`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk#L680-L691)).
- **Effort:** S. **Risk:** none.

### G. Content gaps, in one list

- **Security policy:** C1, C2.
- **Threat model:** C3.
- **Changelog and release notes:** D2, with PR #148.
- **Prelude and `std` API reference:** A5 (*AI-dev* D2).
- **"What works today":** A6.
- **"Coming from Rust or Python" cheat sheet:** *AI-dev* D3.
- **Playground:** B4, deferred.
- **Example pages:** A1, already written but not published.
- **Uninstall and update docs:** *Install* F2, F3 and F11.

## Ranked top 10

The ranking weighs harm today (broken or misleading content first), value to the two audiences the site names (agents and security-minded people), and effort.

1. **A1: link handling and the unpublished examples.** Agents reading the Markdown copies or `llms-full.txt` hit 35 dead targets, and the guides' example links are dead for everyone. Effort S.
2. **C1 + C2: security policy, private reporting, `security.txt`.** A secure-by-default language with no way to report a vulnerability fails the first check an evaluator makes, and Rust and Go both have one. Effort S.
3. **A6 + the README fix: correct the claims and add "What works today".** The home page promises a logging API that does not exist and SQL and process APIs that do not run. Trust lost here costs more than any feature gains. Effort S.
4. **B1 + B2 + B3: the first-run path.** Put the human install, the agent prompt and the quickstart on the first screen, lead the navigation with tasks, and put the installers first on the install page. Effort S–M.
5. **A2 + A3 + D3: tier the agent corpus.** Use "Start here" and `## Optional` in `llms.txt`, slim and reorder `llms-full.txt`, and mark research as non-normative. This uses llmstxt.org's own convention. Effort S.
6. **A5: publish the API reference.** It is the largest remaining gap between "hello world" and real programs for an agent (*AI-dev* D2). Effort M.
7. **C3: the threat-model page.** It makes the authority report and the installer's guarantees interpretable, and records the GitHub Pages limits honestly. Effort M.
8. **D2: release notes and a releases index**, on top of PR #148. Effort S–M.
9. **C4 + C5: key fingerprint in a second channel, then build provenance.** C4 is S and immediate; C5 is M and goes through the release runbook.
10. **The head-and-polish bundle: E1, E3, F1, F2 and C6.** Meta descriptions, a favicon, a custom 404, the search contrast and focus fixes, a skip link, and self-hosted fonts. Each is S, and together they remove the site's only third-party origin.

Just outside the top 10:

- D1, versioned docs. It is valuable once there is more than one supported release, and needs a base-path change first.
- E2, `lastmod`.
- A4, code summaries and `url`.
- C8, repository hygiene.

## Open questions

1. **Home page: README or its own page?** Should `index.html` stay the README, which is also GitHub's front page, or come from a new `docs/home.md` (B1)? *Recommended:* a separate page. The README serves contributors; the site serves users and agents.
2. **Should research be published, and is it in the agent corpus?** Publish it, and exclude it from `llms-full.txt`? *Recommended:* publish it under `## Optional`, with D3's banner, and keep it out of `llms-full.txt`.
3. **The disclosure channel.** Is GitHub private vulnerability reporting alone enough, or should there be an email address too (C1)? *Recommended:* private reporting only, until there are users who cannot use GitHub.
4. **New generator inputs versus workflow steps.** For `lastmod`, `security.txt` and fonts, add inputs to the generator, where they are visible in its authority report, or keep them in `pages.yml`? *Recommended:* text inputs (`lastmod.txt`, the API JSON) in the generator, and binary or dated files (fonts, `security.txt`) in the workflow.
5. **Versioned docs scope (D1).** Every release, or only the latest stable plus `main`? *Recommended:* the latest stable plus `main` while pre-alpha.

## All sources fetched

| Source | Quality | Used for |
| --- | --- | --- |
| [oskalang.dev](https://oskalang.dev/) and its 184 sitemap URLs, `llms.txt`, `llms-full.txt`, `codes.json`, `search.json`, `latest.json`, `robots.txt`, `sitemap.xml`, response headers | primary (subject) | Part 1 |
| [`site/build.osk`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/build.osk), [`site/style.css`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/style.css), [`site/search.js`](https://github.com/aparsons/oska/blob/fec98eccf2d3/site/search.js), [`pages.yml`](https://github.com/aparsons/oska/blob/fec98eccf2d3/.github/workflows/pages.yml), [`release.yml`](https://github.com/aparsons/oska/blob/fec98eccf2d3/.github/workflows/release.yml) | primary (subject) | Part 1, fit |
| [llmstxt.org](https://llmstxt.org/) | primary | Agent consumption |
| [RFC 9116](https://www.rfc-editor.org/rfc/rfc9116.html) | primary | security.txt |
| [W3C CSP Level 3](https://www.w3.org/TR/CSP3/) | primary | Meta-delivered CSP limits |
| [MDN: CSP](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CSP) | primary | Meta-delivered CSP |
| [MDN: Subresource Integrity](https://developer.mozilla.org/en-US/docs/Web/Security/Subresource_Integrity) | primary | SRI scope |
| [W3C: Understanding SC 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) | primary | Accessibility |
| [W3C: Understanding SC 2.4.1](https://www.w3.org/WAI/WCAG22/Understanding/bypass-blocks.html) | primary | Accessibility |
| [Google: snippets and meta descriptions](https://developers.google.com/search/docs/appearance/snippet) | primary | SEO |
| [Google: build a sitemap](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap) | primary | SEO |
| [Google: software app structured data](https://developers.google.com/search/docs/appearance/structured-data/software-app) | primary | Structured data |
| [schema.org/SoftwareSourceCode](https://schema.org/SoftwareSourceCode) | primary | Structured data |
| [Google Registry: .dev](https://www.registry.google/domains/dev/) | primary | HSTS preload |
| [OpenSSF Scorecard checks](https://github.com/ossf/scorecard/blob/main/docs/checks.md) | primary | Trust signals |
| [SLSA v1.0 levels](https://slsa.dev/spec/v1.0/levels) | primary | Provenance |
| [GitHub Docs: artifact attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) | primary | Provenance |
| [GitHub Docs: private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/working-with-repository-security-advisories/configuring-private-vulnerability-reporting-for-a-repository) | primary | Security policy |
| [GitHub Docs: custom 404](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-custom-404-page-for-your-github-pages-site) | primary | 404 page |
| [GitHub Docs: HTTPS for Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/securing-your-github-pages-site-with-https) | primary | Header limits |
| [Rust security policy](https://www.rust-lang.org/policies/security) | primary | Prior art |
| [Rust 1.80.0 std docs](https://doc.rust-lang.org/1.80.0/std/) | primary | Versioned docs |
| [Go security policy](https://go.dev/doc/security/policy) | primary | Prior art |
| [Go release history](https://go.dev/doc/devel/release) | primary | Release notes |
| [Zig download](https://ziglang.org/download/) | primary | Key publication, versioned docs |
| [Gleam](https://gleam.run/) | primary | First-run path |
| [Roc](https://www.roc-lang.org/) | primary | First-run path, playground |
| [Deno security and permissions](https://docs.deno.com/runtime/fundamentals/security/) | primary | Threat-model prior art |
