# Research: designing a secure-by-default, AI-first programming language

- **Date:** 2026-11-08
- **Method:** An automated deep-research workflow. It split the question into 5 search angles, fetched 23 sources and extracted 108 claims. The top 25 claims were each checked by 3 adversarial verifiers, and a claim was dropped if 2 of the 3 refuted it. 22 claims were confirmed and 3 were refuted, and the confirmed claims were merged into 8 findings.
- **Question:** What modern best practices in programming language design, security, and software design are needed to build a new AI-first language? The language must be secure by default and optimized for AI-agent-driven development, so that agents can easily write secure software.

## Summary

The strongest evidence points to four foundations for a secure-by-default, AI-first language:

1. **Memory safety as a guarantee that cannot be switched off.** Escape hatches are kept narrow and clearly marked.
2. **Object capabilities with no ambient authority.** There are no globals and no unrestricted filesystem or network access. Code gets that access only when a capability is passed in explicitly.
3. **Effects and requirements declared in every function signature and checked statically.**
4. **Machine-checkable contracts, plus language and library designs that make whole vulnerability classes impossible to write** (for example, SQL injection). Code written by an agent is then verified rather than trusted.

Benchmarks show why this matters. LLMs routinely write code that works but is insecure. On CWEval, pass rates drop about 30% when security is also required. 45% of samples failed Veracode's tests, and 73-90% of snippets were vulnerable on prompts targeting specific CWEs. Safety therefore has to come from the language and its tooling, not from the model.

Experimental AI-first languages (Vera, Nexus) share one premise: models do best when everything relevant (effects, dependencies, contracts, binding structure) is explicit and local in the source text. **That premise comes from the language authors and has not been measured.** The capability and effect mechanisms themselves are well established, in Pony and in academic work (Callℇ).

## Findings

### 1. Make memory safety a non-negotiable baseline. (Confidence: high, vote 3-0)

Government guidance names memory-safe languages (MSLs) as the most complete mitigation for the bug class behind about 70% of serious vulnerabilities. It recommends writing all new code in an MSL.

- CISA/NSA (2025): adopting MSLs "offers the most comprehensive mitigation against this class of vulnerabilities."
- *The Case for Memory Safe Roadmaps* (Dec 2023):
  - About 70% of Microsoft CVEs (2006-2018) were memory-safety issues.
  - About 70% of Chromium's high and critical security bugs were memory-safety issues.
  - Writing new projects in an MSL "puts a cap" on memory-safety vulnerabilities.
- Google's Android data, cited by one verifier: memory-safety vulnerabilities fell from about 76% to about 24% as new code moved to MSLs.

Sources: [CISA 2025 alert](https://www.cisa.gov/news-events/alerts/2025/06/24/new-guidance-released-reducing-memory-related-vulnerabilities), [The Case for Memory Safe Roadmaps](https://cisa.gov/resources-tools/resources/case-memory-safe-roadmaps)

### 2. Escape hatches are where guarantees break down, so restrict them and mark them in the syntax. (Confidence: high, vote 3-0)

Unsafe blocks and FFI are where memory and capability guarantees fail. A secure-by-default language should:

- allow them only where they are needed, such as OS interaction
- mark them in the syntax
- flag them
- allow them only in specific places, for example per package

Evidence:

- CISA: "it is possible to defeat the memory protection guarantees in some MSLs", for example with Rust's `unsafe`. Its use should be limited to cases such as direct OS interaction.
- Pony's docs state that its C-FFI can break the unforgeable-reference and capability guarantees. Pony partly contains this with `--safe`, which restricts FFI to packages on an allow-list.

*The design recommendation is inferred from these sources. They do not state it directly.*

Sources: [CISA roadmaps](https://cisa.gov/resources-tools/resources/case-memory-safe-roadmaps), [Pony tutorial: object capabilities](https://tutorial.ponylang.org/capabilities/object-capabilities.html), [Pony patterns: object capabilities](https://patterns.ponylang.io/object-capabilities/)

### 3. Use object capabilities and eliminate ambient authority. (Confidence: high, vote 3-0)

- References cannot be forged.
- There are no global variables or functions.
- Authority comes from holding a reference or token, not from checks against an access-control list.

Evidence:

- Pony has no pointer arithmetic and is type- and memory-safe, so "object references can't be invented (i.e. forged)". Code can get a reference only by constructing an object or being passed one.
- Globals and unrestricted filesystem access are both forms of ambient authority. Pony omits globals, but "having no global variables is necessary, but not sufficient."

Source: [Pony tutorial: object capabilities](https://tutorial.ponylang.org/capabilities/object-capabilities.html)

### 4. Structure authority as a single-root hierarchy that the compiler enforces and that only flows downward. (Confidence: high, vote 3-0)

`main` receives the root token and hands narrower tokens to components, so each component gets least authority by construction.

- In Pony, `AmbientAuth` is the only root, and the runtime gives it only to `Main` (as `env.root`). It grants access to the filesystem, process spawning, and other system resources.
- Narrower tokens are built from broader ones:
  - `NetAuth` from `AmbientAuth`
  - `TCPListenAuth` from `AmbientAuth`, `NetAuth`, or `TCPAuth`
- Authority "flows downward… never sideways."
- The compiler enforces this: "If you don't have a value of the right type to pass to the constructor, your code won't compile."

Sources: [Pony patterns: object capabilities](https://patterns.ponylang.io/object-capabilities/), [Pony patterns: authority hierarchy](https://patterns.ponylang.io/object-capabilities/authority-hierarchy/)

### 5. Declare effects in signatures and check them statically. (Confidence: medium, vote 3-0)

A type-and-effect system can stop code from performing privileged operations (database, network, UI, process exit) or non-deterministic operations unless they are declared. The annotation burden can be kept low.

- **Callℇ** (Onward! 2019):
  - It blocks privileged operations and enforces determinism by banning transitive calls to non-deterministic primitives.
  - Effects are method names, and each method declares an upper bound on its effects.
  - The Indirect-Callℇ rule lets a method annotated `effect[ε]` count as a sub-effect of ε. This keeps annotations short and hides implementation details.
- **Vera:** functions are pure by default. Typed effects (`<Http>`, LLM calls) appear in the signature, and a caller must permit them.
- **Nexus:** uses coeffects.
  - `require { ... }` declares the capabilities a function needs.
  - Handlers are plain values injected into a scope, similar to dependency injection.
  - There are no continuations and no implicit transfer of control.

*Confidence is medium because Callℇ is a research prototype and Vera and Nexus are small experimental projects.*

Sources: [Callℇ (arXiv 1907.04934)](https://arxiv.org/pdf/1907.04934), [Vera](https://github.com/aallan/vera), [Nexus](https://github.com/nymphium/Nexus)

### 6. Make contracts mandatory and machine-checked, and make common vulnerability classes impossible to write. (Confidence: medium, vote 3-0)

The point is that agent-generated code only has to be checkable, not correct on the first try.

- In Vera, every function must declare `requires`, `ensures`, and `effects` clauses.
- Contracts are checked in tiers:
  - Tier 1: Z3 proves decidable contracts statically.
  - Undecidable contracts fall back to runtime checks.
  - Tier 2, which would be guided by Z3, is specified but not implemented.
- Design motto: *"The model doesn't need to be right, it needs to be checkable."*
- SQL text must be a compile-time literal. Outside data can enter only through `?` placeholders, and error E207 flags queries built from strings.
- `vera verify` rejects possible integer division by zero and indexing that is provably out of bounds.

*Caveats:*

- Some runtime checks are documented but not compiled in.
- Float division is exempt from the division-by-zero check.
- This finding rests on a single source, an early-stage project.

Source: [Vera](https://github.com/aallan/vera)

### 7. Two AI-first design premises that are untested. (Confidence: low, vote 3-0 that the projects make these claims)

The premises:

- Put every dependency, effect, and resource state explicitly on the page.
- Avoid coherence problems that come from naming.

Evidence:

- Nexus README: "LLMs are good at code you can read on the page, and bad at code that depends on what's off the page… Implicit control flow, hidden state, and ambient authority are where LLMs go wrong."
- Nexus is small (7 stars, no releases) and cites no benchmark.
- Vera has no variable names. Bindings are referred to by their position among bindings of the same type, as typed De Bruijn indices: `@Int.0` is the most recent `Int` binding.
- Vera's rationale is that models fail at staying coherent across large code, not at syntax.

**Neither project offers evidence that these choices improve what LLMs produce.**

Sources: [Vera](https://github.com/aallan/vera), [Nexus](https://github.com/nymphium/Nexus)

### 8. LLMs consistently produce code that works but is insecure. (Confidence: high, vote 3-0)

A large share of generated code contains high-severity CWE and OWASP vulnerabilities. A language meant for agents should make the secure path the default rather than relying on the model.

| Benchmark | Scope | Key findings |
| --- | --- | --- |
| CWEval (Jan 2025) | 119 tasks, 31 CWEs, 5 languages | Pass rates drop about 30% from functional-only (func@10) to functional and secure (func-sec@10). The largest drop is 35.79%, for Gemini 1.5 Flash. GPT-4o scores 90.71 vs 65.33 at k=10 and 80.81 vs 50.21 at k=1. |
| Veracode (Jul 2025) | More than 100 LLMs | 45% of samples failed security tests with OWASP Top 10 vulnerabilities. |
| Morkonda et al. (2026) | 7 tools, 81 LLMSecEval Python prompts | 73-90% of snippets per tool had at least one vulnerability. There were 995 vulnerabilities across 567 snippets: 65% High and 33% Critical. |

Sources: [CWEval (arXiv 2501.08200)](https://arxiv.org/pdf/2501.08200), [Veracode GenAI code security report](https://www.veracode.com/resources/genai-code-security-report/), [Morkonda et al. (arXiv 2605.23091)](https://arxiv.org/pdf/2605.23091)

## Refuted claims (excluded)

| Claim | Vote | Source |
| --- | --- | --- |
| Nexus checks capabilities twice: at build time, and at runtime through WASI permissions such as `PermNet` and `PermConsole`. | 1-2 | [Nexus docs](https://nymphium.github.io/Nexus/latest) |
| LLM security performance did not improve with model size or newer training. | 0-3 | [Veracode](https://www.veracode.com/resources/genai-code-security-report/) |
| The most common weakness was CWE-200, and 69% of those instances came from Flask debug mode. | 0-3 | [arXiv 2605.23091](https://arxiv.org/pdf/2605.23091) |

## Caveats

- **The AI-first language evidence is thin.** It comes from the authors' own READMEs for small experimental projects:
  - Vera: the spec is a draft and Tier 2 is unimplemented.
  - Nexus: 7 stars, no releases, and the docs URLs no longer work.

  Their claims that LLMs do better with explicit or nameless code are design rationale, not measured results.
- **Callℇ is a 2019 research prototype**, not a production language.
- **Pony's guarantees do not hold across its C-FFI.**
- **The LLM security benchmarks have limits:**
  - They test 2024-era models (GPT-4o, Claude 3, Gemini 1.5), and current agentic models may score differently.
  - Prompts were often built around specific CWEs, which pushes vulnerability rates higher than typical code would show.
  - Veracode sells security tooling, so its study is a vendor benchmark.
  - Morkonda et al. used GPT-4o both as one of the tested models and as a detector.
  - CWEval does not say whether its ~30% drop is relative or absolute.
- **Some government data is old.** The memory-safety figures cover 2006-2020, though newer Android data supports the same conclusion.
- **Some recommendations are inferences rather than statements in the sources.** These are "restrict escape hatches" and "make safety come from the language, not the model".
- **Two angles had no verified claims.** Sources were fetched for the "Agent tooling and feedback loops" and "Skeptical and tradeoffs view" angles. None of their claims were among the 25 verified, because verification was capped at the top 25 claims. Those angles are therefore not represented in the findings above. Their sources are listed below for follow-up.

## Open questions

1. Do explicit effects, capabilities, mandatory contracts, or nameless bindings (De Bruijn indices) actually improve the security or correctness of code that LLM agents write, compared with conventional languages? No controlled study was found.
2. How should strong static verification (SMT-backed contracts, effect typing) be balanced against how easily models learn the language? A new language has almost no training data. Can compiler error messages, docs, or a constrained grammar make up for that?
3. How should unsafe and FFI boundaries, and interop with existing ecosystems, be designed so that capability and memory-safety guarantees are not quietly undermined?
4. Beyond SQL injection, which vulnerability classes common in LLM output can language or standard-library design make impossible to write? Candidates include XSS, log injection, exposed secrets, and insecure defaults. Which still need runtime or analysis tooling?

## Relevance to Oska

This section is an editorial mapping onto [`docs/design.md`](../design.md). It is not a research finding.

**Already aligned with the high-confidence findings:**

- memory safety
- no ambient authority, with capabilities passed as values
- signatures that say everything a function can do
- `Untrusted<T>` taint tracking

**Gaps worth considering:**

- **Unsafe and FFI policy.** How are escape hatches marked, flagged in tooling, and restricted per package? (Finding 2)
- **Capability hierarchy.** Should `main` receive one root capability from which narrower ones (`FsRead`, `Net`, …) are derived, Pony-style? (Finding 4)
- **Contracts.** Should `requires`/`ensures` be added, and if so, are they mandatory or optional? Should they be checked statically by SMT or at runtime? (Finding 6)
- **Sink-specific APIs.** Should there be APIs that make whole vulnerability classes impossible to write, for example SQL that only accepts compile-time literals with parameters? These would complement the `Untrusted<T>` taint tracking. (Findings 6 and 8; open question 4)
- **Evaluation.** The AI-first premises are untested, so Oska could measure them itself. For example, it could run CWEval-style tasks against agents writing Oska. (Finding 7; open question 1)

## All sources fetched

| Source | Quality | Angle |
| --- | --- | --- |
| [Vera](https://github.com/aallan/vera) | primary | LLM-oriented language design |
| [AILang: why AILang](https://sunholo-data.github.io/ailang/docs/why-ailang) | blog | LLM-oriented language design |
| [Nexus docs](https://nymphium.github.io/Nexus/latest) | primary | LLM-oriented language design |
| [Akita: best programming language for LLMs](https://akitaonrails.com/en/2026/02/09/ai-agents-best-programming-language-for-llms/) | blog | LLM-oriented language design |
| [Why we built ll-lang](https://dev.to/neftedollar/why-we-built-ll-lang-a-statically-typed-functional-language-for-llms-2hg8) | blog | LLM-oriented language design |
| [LLM-friendly language](https://lordajax-com.vercel.app/llm-friendly-language) | unreliable | LLM-oriented language design |
| [CISA 2025 memory-safety alert](https://www.cisa.gov/news-events/alerts/2025/06/24/new-guidance-released-reducing-memory-related-vulnerabilities) | primary | Secure-by-default features |
| [The Case for Memory Safe Roadmaps](https://cisa.gov/resources-tools/resources/case-memory-safe-roadmaps) | primary | Secure-by-default features |
| [Pony tutorial: object capabilities](https://tutorial.ponylang.org/capabilities/object-capabilities.html) | primary | Secure-by-default features |
| [SecurityWeek: CISA secure-by-design](https://www.securityweek.com/cisa-introduces-secure-by-design-and-secure-by-default-development-principles/) | secondary | Secure-by-default features |
| [Callℇ (arXiv 1907.04934)](https://arxiv.org/pdf/1907.04934) | primary | Secure-by-default features |
| [Pony patterns: object capabilities](https://patterns.ponylang.io/object-capabilities/) | primary | Secure-by-default features |
| [CWEval (arXiv 2501.08200)](https://arxiv.org/pdf/2501.08200) | primary | AI-generated code vulnerabilities |
| [Veracode GenAI code security report](https://www.veracode.com/resources/genai-code-security-report/) | primary | AI-generated code vulnerabilities |
| [arXiv 2605.23091](https://arxiv.org/pdf/2605.23091) | primary | AI-generated code vulnerabilities |
| [arXiv 2506.23034](https://arxiv.org/pdf/2506.23034) | primary | AI-generated code vulnerabilities |
| [arXiv 2604.13927](https://arxiv.org/pdf/2604.13927) | primary | Agent tooling and feedback loops |
| [arXiv 2510.22907](https://arxiv.org/pdf/2510.22907) | primary | Agent tooling and feedback loops |
| [arXiv 2602.11481](https://arxiv.org/pdf/2602.11481) | primary | Agent tooling and feedback loops |
| [arXiv 2410.03981](https://arxiv.org/html/2410.03981v3) | primary | Skeptical and tradeoffs view |
| [arXiv 2501.19085](https://huggingface.co/papers/2501.19085) | primary | Skeptical and tradeoffs view |
| [Can a new language survive when LLMs only know the old ones?](https://news.lavx.hu/article/can-a-new-programming-language-survive-when-llms-only-know-the-old-ones) | secondary | Skeptical and tradeoffs view |
| [Akita: are there better languages for programming with LLMs?](https://akitaonrails.com/en/2026/10/11/are-there-better-languages-for-programming-with-llms/) | blog | Skeptical and tradeoffs view |
