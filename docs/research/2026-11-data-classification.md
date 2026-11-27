# Could Oska support data classification (labels such as public / internal / confidential / restricted, enforced by the language so labelled data cannot leak to the wrong sink)? How?

Date of access: 2026-11-26

Method: repo ADRs and glossary read in full; external sources fetched over HTTPS. Claims marked "Inference:" are mine. Sources that could not be read are listed under "Not verified". The question is not fully closed: see section 4.

## Short answer

Inference: Yes. Oska's trust flag is already a two-point information-flow lattice with a one-way flow rule, and capabilities already name every sink. A small classification lattice can reuse both. The cheap, sound part is explicit flows checked statically at named sinks. Implicit flows, timing and declassification policy are the hard part, and the language should say plainly that it does not cover them at first.

## 1. What Oska has today

- **Trust flag on text.** `Untrusted<String>` is a flag on text types, tracked by one-way flow: trusted may flow to untrusted slots, never the reverse. Prelude text operations propagate it, so one untrusted input makes every text type in the result untrusted. Non-text results (`Int`, `Bool`) are trusted. (docs/adr/0006-trust-is-a-flag-on-text.md)
- **Why a flag and not a wrapper type.** Oska has no subtyping and no overloading, and a wrapper would duplicate every text method. (docs/adr/0006-trust-is-a-flag-on-text.md)
- **Explicit downgrade is an audit site.** `.assume_trusted()` is a trust assertion, listed beside `Secret.expose()`. (docs/adr/0006-trust-is-a-flag-on-text.md, GLOSSARY.md "Trust assertion", "Audit site")
- **Sinks already split into structural and data positions.** A data position accepts untrusted values; a structural position rejects them (GLOSSARY.md, "data position" and "structural position" entries). A classification check would add a third rule to the same machinery.
- **Capabilities are the effect system.** A function's effects are the capabilities reachable through its parameters. A function with no capability-bearing parameters is pure. (docs/adr/0002-capabilities-are-the-effect-system.md)
- **Sinks are narrowed capabilities.** Files go through `DirRead`/`DirWrite`/`FileRead`/`FileWrite` (docs/adr/0008-file-access-goes-through-confined-capabilities.md). The network goes through `HostConnect`, confined to one host and port, and everything read from a connection is `Untrusted` (docs/adr/0025-network-access-goes-through-host-narrowed-capabilities.md). `std.llm` is driven by a budget capability (GLOSSARY.md, budget entry).
- **Authority is reported and bounded.** Unused authority is a release-check error (docs/adr/0016-unused-authority-is-a-release-check-error.md). Widening of authority must be approved (GLOSSARY.md, "Widening"). Lean proves authority safety for Oska Core (docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md).
- **Secret values.** A secret value is never displayed, logged, interpolated or compared with `==`. Secret integers (`Secret<U8/U32/U64>`) allow constant-time computation and forbid branching, indexing, dividing, shifting or ordering on them. The property holds for native code, not for the interpreter, and not for what hardware does (GLOSSARY.md, "Secret value", "Secret integer", constant-time entry). Inference: this is a one-level confidentiality label with a built-in sink rule (no log, no interpolation) and a built-in ban on the main implicit flows (branch, index).
- **Compliance reports are self-assessments**, with findings rated by risk and a determination (docs/adr/0017-compliance-reports-are-self-assessments.md).
- **Parity gate.** ADR 0014 froze new language features until every supported platform had a native backend, then turns on a parity gate: a feature is done only when it works natively, with the same output and security guarantees, on every supported platform. The ADR names v0.6.0 (Windows) as the release that turns the gate on (docs/adr/0014-every-supported-platform-builds-and-runs-natively.md). Inference: v0.6.0 is released, so the freeze is over and a classification feature must meet the parity gate on all five platforms. The GitHub release v0.6.0 was published on 2026-11-20 (https://github.com/aparsons/oska/releases/tag/v0.6.0, accessed 2026-11-26).
- **Context.** The earlier research lists "exposed secrets" and log injection among classes to make unwritable, and asks for sink-specific APIs (docs/research/2026-11-ai-first-secure-language.md, question 4 and its list of `Untrusted<T>` taint tracking).

## 2. What the literature and existing tools say

- **Denning's lattice model (1976).** Not verified: I could not fetch the paper. Treated here only as the origin of the idea of labels forming a lattice with "may flow to" as the order; the survey below is the source I actually read.
- **Noninterference and flow kinds.** Noninterference means varying high input must not vary low output. `l := h` is an explicit flow; `if h = 1 then l := 1` is an implicit flow, the harder case. Covert channels include termination, timing, probabilistic, resource and power channels. (Sabelfeld and Myers, "Language-Based Information-Flow Security", https://www.cs.cornell.edu/andru/papers/jsac/sm-jsac03.pdf)
- **Static vs dynamic.** Dynamic monitors suffer "label creep"; static labels "are not computed at run time", and static checking handles implicit flows more precisely. Pure noninterference rejects all downgrading, so controlled declassification is needed. (same survey)
- **Decentralized label model.** A label is a set of owner:readers policies, all of which must hold. Code running with an owner's authority can relax that owner's policy only. A principal that acts for another has its powers. (Myers and Liskov, https://www.cs.cornell.edu/andru/papers/iflow-tosem.pdf)
- **Jif.** Extends Java with labels on types (`int {Alice->Bob} x;`), checked at compile time, then translated to Java. It lists "selective, robust downgrading" as a feature. The page gives no detail of analysis precision. (https://www.cs.cornell.edu/jif/)
- **FlowCaml and LIO.** Not verified: I could not read either first-party page (the Hackage and Chalmers fetches failed). The Jif page names Flow Caml only as a related system.
- **Java Checker Framework.** The manual's tainting chapter exists (sections on `@Tainted`/`@Untainted`, tips, polymorphism and side effects), but the fetch cut the body, so I did not read its rules. The manual lists SPARTA and SFlow as third-party information-flow type checkers. (https://checkerframework.org/manual/#tainting-checker)
- **Perl taint mode.** Taint attaches to each scalar; any expression containing tainted data is tainted; refused operations include `system`, `exec`, write-`open` and `unlink`; `print` and `syswrite` are not checked; untainting is by regex capture. The ternary operator is the only stated exception to conservative propagation. The document says taint mode prevents "stupid mistakes" and is not a sandbox. (https://perldoc.perl.org/perlsec) Inference: Oska's design is closer to Perl's flag than to Jif's labels, but with a static check and an audited downgrade, and Perl's unchecked `print` is the exact gap that classification must not copy.
- **Rust type-state, Go, Ruby taint.** Not covered: I did not read a first-party doc for them within the source budget.

## 3. Label names and what they map to

- **FIPS 199** categorizes information by level of concern for confidentiality, integrity and availability, tied to impact if compromised. The page does not define low/moderate/high; those are in the linked PDF, which I did not read. (https://csrc.nist.gov/pubs/fips/199/final) Inference: public/internal/confidential/restricted is a four-point confidentiality scale, so it does not map one-to-one onto three impact levels, and FIPS 199 is about confidentiality, integrity and availability together.
- **TLP 2.0** has four sharing labels: CLEAR (no disclosure limits), GREEN (community, not public channels), AMBER (need-to-know in the organization and its clients; AMBER+STRICT organization only), RED (named recipient only). (https://www.first.org/tlp/) Inference: TLP is a sharing protocol with an ordered lattice, so it fits the same mechanism with different names.
- **ISO/IEC 27001 Annex A 5.12-5.13 (classification and labelling of information)** and **GDPR Art. 9 (special categories)**: not verified. Both are paywalled or the fetch failed. GDPR Recital 51 says special-category data should not be processed unless a specific case in the Regulation allows it (https://eur-lex.europa.eu/legal-content/EN/TXT/HTML/?uri=CELEX:32016R0679), but I did not read Article 9(1)'s list. Inference: GDPR special categories are a property of the data's subject matter, which is not an order on a lattice, so they suggest a second axis (a tag) and not another rung.
- Inference: because standards disagree on names and count, Oska should define the lattice in the program (or per package), not hard-code four names.

## 4. Design options for Oska (all Inference)

**A. A classification flag on text and bytes, like trust.**
A small ordered set, written `Classified<String, Confidential>` in the same non-generic way `Untrusted<String>` is. Flow is one-way upward (public may flow to confidential, not the reverse). Prelude operations join labels (result has the highest input label). Non-text results from inspection keep the rule ADR 0006 uses for trust; for classification this is the first place to decide (open question 3). This reuses the checker mechanism of ADR 0006 and costs no duplicated API. It cannot express owners or readers.

**B. Labels on any type, not only text.**
Carrying labels on `Int`, structs and lists needs the label to travel through fields and generics. ADR 0006 rules out a wrapper for lack of subtyping; a struct field flag would need the same trick extended. Larger and riskier. Not recommended first.

**C. A full decentralized label model (owners and readers, acts-for).**
Gives a real policy language (Myers and Liskov), but needs principals, a runtime or static principal hierarchy and label polymorphism. Too heavy for Oska's minimal-concept style.

**D. Labels as capabilities attached to sinks.**
Each sink capability carries a clearance. `Stdout`, a `DirWrite`, a `HostConnect` and a `std.llm` client each have a ceiling label. A write is accepted only if the value's label is at or below the ceiling. Attenuation can only lower a ceiling, matching how `restrict_to` only narrows (docs/adr/0008-file-access-goes-through-confined-capabilities.md, docs/adr/0025-network-access-goes-through-host-narrowed-capabilities.md). `main` would receive a ceiling in its signature, so the authority report shows it. Widening a ceiling joins the existing widening approval (GLOSSARY.md, "Widening").

**E. Declassification as a capability or audit site.**
Two choices. (1) An audit site like `.assume_trusted()`, listed by `oskac audit`. (2) A `Declassify` capability passed down from `main`, so a package without it cannot lower a label at all. Option (2) matches ADR 0002 (authority is what you hold) and parallels Jif, where code with an owner's authority may relax that owner's policy (Myers and Liskov). Recommended: (2), with every use also an audit site.

**F. Sinks.**
Checked statically: logging and `dbg`, `print`, string interpolation into a lower-labelled value, file writes, network sends, `std.llm` prompts, process arguments, and `Secret` exposure. Inference: `Secret<T>` could become the top label, unifying the two ideas. Reads from a capability get the capability's own label (a `DirRead` on a confidential directory yields confidential text), mirroring ADR 0025's rule that everything read from a connection is `Untrusted`.

**G. Interpreter, native backends, Lean.**
Labels are erased after checking, like trust is a checker-time property in ADR 0006, so the interpreter and native backends need no runtime label and parity (docs/adr/0014-every-supported-platform-builds-and-runs-natively.md) is cheap. The Lean reference (docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md) says proofs of trust rules come later on the same calculus; a noninterference proof for classification would be the natural companion and would be a larger job than the authority-safety proof. Any runtime-visible behaviour (a `Declassify` call, labelled values in test output) must run the same on every backend. Timing is different: ADR glossary entries say constant-time holds for native code, not the interpreter (GLOSSARY.md, constant-time entry), so classification must not promise timing safety.

**H. Compliance reports.**
The authority report can list each sink's ceiling, each declassification site and each label widening. Per ADR 0017 these are self-assessment evidence, not an audit. A map from lattice names to FIPS 199, TLP or ISO 27001 controls would be a documented convention, and a report must not claim a classification scheme was certified.

## 5. Recommended minimal first step (Inference)

1. Add one new flag on text and bytes, written like `Untrusted<String>`, with a lattice fixed to four ordered names for the first release: `Public < Internal < Confidential < Restricted`. Same one-way flow, same join on prelude text operations.
2. Give a ceiling to exactly three sinks first: logging/`dbg`/printing, network send (`HostConnect`), and `std.llm`. Default ceiling for each is `Public`, so confidential data cannot reach them unless the signature says otherwise. File sinks follow once the first three are stable.
3. Declassification only by a `Declassify` capability from `main`, and every use an audit site.
4. Make `Secret<T>` documentation say it is the top of the same order, but do not change its behaviour yet.
5. State in the spec that implicit flows, timing and termination channels are out of scope; reject the cheapest ones (branching on a labelled `Bool` converted from text is already trusted-by-inspection under ADR 0006, so decide this explicitly, see open question 3).

**Costs.** One checker flag and join rule, a ceiling field on four capability kinds, authority-report columns, an ADR, a glossary entry, Lean model update in the same PR (docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md), and tests on every backend. Inference: roughly comparable to adding the trust flag, plus the ceiling work. Not estimated in time.

**Risks.**
- **Label creep.** With join-on-every-operation, results become highly labelled and need declassifying to be useful; the survey names this for dynamic monitors, and static labels reduce but do not remove the pressure (Sabelfeld and Myers). Mitigation: only text and bytes carry labels, and non-text results from inspection stay unlabelled.
- **Implicit flows.** `if secret_text.contains("x") then log("yes")` leaks one bit with no text copied. Under ADR 0006's rule that inspection yields trusted results, the same rule would make this pass for classification. This is the largest hole and is a policy decision (open question 3).
- **Timing and secret integers.** Constant-time guarantees hold only for native code (GLOSSARY.md). A label system that says "cannot leak" would overclaim.
- **Declassifier fatigue.** If agents add `Declassify` to make errors go away, the check is empty. ADR 0016 shows the answer Oska prefers: make the authority visible and release-checked, with no escape hatch name.
- **Parity gate.** The feature must work natively with the same output and guarantees on all five platforms (ADR 0014). The freeze itself ended with v0.6.0.
- **Overclaiming to auditors.** Label names borrowed from a standard do not make a program conform to it (docs/adr/0017-compliance-reports-are-self-assessments.md).

## 6. Open questions for the maintainer

1. ADR 0014's freeze ended with v0.6.0, so this needs no exception, only the parity gate. Confirm that reading.
2. Fixed four-name lattice, or a per-package lattice? How do TLP-style (sharing) and sensitivity-style names coexist?
3. Does a non-text result computed from classified text (length, `contains`, `parse_int`) stay unlabelled, as trust does, accepting implicit flows, or is it labelled too (more sound, much more label creep)?
4. Is `Secret<T>` the top label, a separate axis, or unrelated?
5. Should a single value carry both trust and classification, and how do the two interact at `std.llm` (untrusted input into a model with confidential context)?
6. Declassification: audit site only, a `Declassify` capability, or a validator-style function with a typed result?
7. Do labels apply to bytes, and to structured data read from JSON or a database?
8. Should a capability's ceiling be set by the program, by the caller, or by the authority gate (package-level approval)?
9. Is the Lean proof of noninterference a release blocker for the feature, as ADR 0018 requires for typing changes?
10. Which standard (if any) should the reports map labels to? ISO/IEC 27001 Annex A 5.12-5.13 and GDPR Art. 9 were not read.

## Not verified

- Denning (1976), FlowCaml, LIO: no first-party page read.
- Checker Framework tainting rules: chapter body not retrieved.
- FIPS 199 category definitions, ISO/IEC 27001 Annex A 5.12-5.13, GDPR Art. 9(1) list: not read.
- Rust type-state, Go and Ruby taint documentation: not read.

## Sources

Repo (read in full unless noted):
1. docs/adr/0006-trust-is-a-flag-on-text.md
2. docs/adr/0002-capabilities-are-the-effect-system.md
3. docs/adr/0008-file-access-goes-through-confined-capabilities.md
4. docs/adr/0016-unused-authority-is-a-release-check-error.md
5. docs/adr/0017-compliance-reports-are-self-assessments.md
6. docs/adr/0025-network-access-goes-through-host-narrowed-capabilities.md
7. docs/adr/0014-every-supported-platform-builds-and-runs-natively.md (opening section)
8. docs/adr/0018-oska-core-in-lean-is-the-reference-for-the-interpreter.md (opening section)
9. GLOSSARY.md (searched for capability, trust, secret, sink entries)
10. docs/research/2026-11-ai-first-secure-language.md (skimmed)

External:
11. https://www.cs.cornell.edu/andru/papers/jsac/sm-jsac03.pdf (Sabelfeld and Myers survey)
12. https://www.cs.cornell.edu/andru/papers/iflow-tosem.pdf (Myers and Liskov, decentralized label model)
13. https://www.cs.cornell.edu/jif/ (Jif)
14. https://perldoc.perl.org/perlsec (Perl taint mode)
15. https://checkerframework.org/manual/#tainting-checker (Checker Framework manual, partial)
16. https://csrc.nist.gov/pubs/fips/199/final (FIPS 199, abstract only)
17. https://www.first.org/tlp/ (Traffic Light Protocol)
18. https://eur-lex.europa.eu/legal-content/EN/TXT/HTML/?uri=CELEX:32016R0679 (GDPR, recital 51 only)

## Outcome of the grilling, 2026-11-26

This section was added after the note above and does not change it. Adam settled these points on decision cards in the project thread, and they replace the matching recommendations and open questions above. The record is `docs/adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md`, and the spec is issue #770.

- The four names are fixed by the language, so the line in section 3 that says to define the lattice per program is not followed. A per-package order may come later.
- `Secret` stays a separate axis, so the suggestion in section 4F that `Secret<T>` become the top label is not followed.
- Results of inspecting classified text stay unclassified (open question 3). Trust and classification are two independent flags (question 5). Declassification needs a `Declassify` capability (question 6). Ceilings are raised only by a widening in the authority baseline (question 8). The Lean model proves the flow rule and ceiling check now, and a noninterference proof is a follow-up (question 9). No standard mapping yet (question 10). Parsed text fields keep the flag, with `std.json` and `std.sql` later (question 7).
- Question 1 is answered: the freeze ended with v0.6.0.
