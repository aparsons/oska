# How could the Oska programming language support the building, operating, and maintenance of knowledge graphs and ontologies?

Date of access: 2026-11-27

Every claim about a standard cites the page that owns it. Claims about Oska cite the repository's own docs ([GLOSSARY.md](../../GLOSSARY.md), [ADR 0029](../adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md)). Lines that start with "Inference:" are this note's own conclusions, not facts from a source. Sizes are rough guesses in lines of Oska, not measured.

## Short answer

Inference: Oska is a good host for a knowledge-graph toolchain at the edges (ingest, validate, sign, audit) and a poor candidate for the core (reasoner, large store) for now. Its untrusted-text flag, capabilities, `Budget`, and `Map` with insertion order already match the three hard problems of LLM-built graphs: untrusted extraction, bounded authority, and reproducible output. Start with a strict N-Triples/N-Quads codec, a graph value type, a SHACL-core validator, and canonical hashing. Do not build an OWL reasoner or a SPARQL engine first.

## 1. The standards that own the facts

| Area | Fact | Source |
|---|---|---|
| RDF 1.1 | A graph is a set of triples (subject: IRI or blank node; predicate: IRI; object: IRI, literal or blank node). A dataset is one default graph plus zero or more named graphs. Two graphs are isomorphic when a mapping of blank nodes preserves every triple. | [RDF 1.1 Concepts](https://www.w3.org/TR/rdf11-concepts/) |
| RDF 1.2 | Still a Candidate Recommendation Draft (16 Nov 2026), so not final. It adds triple terms (a triple as an object) and directional language-tagged strings. | [RDF 1.2 Concepts](https://www.w3.org/TR/rdf12-concepts/) |
| RDFS | Gives `rdfs:Class`, `subClassOf`, `domain`, `range`. Domain and range say resources "are instances" of a class. Whether an application checks or infers is left open. | [RDF Schema 1.1](https://www.w3.org/TR/rdf-schema/) |
| OWL 2 | Ontology language with formal meaning. Profiles EL (polynomial time), QL (LogSpace, relational access) and RL (polynomial time, rule engines over RDF). DL is a restriction that maps to SROIQ; Full has no restrictions. | [OWL 2 Overview](https://www.w3.org/TR/owl2-overview/) |
| SHACL | Recommendation of 20 Jul 2017. Validates a data graph against a shapes graph (node and property shapes). The report is RDF, with `sh:conforms` true only when there are no results. | [SHACL](https://www.w3.org/TR/shacl/) |
| SPARQL 1.1 | Recommendation of 21 Mar 2013. Forms SELECT and CONSTRUCT (ASK and DESCRIBE also exist). Property paths `^ / | * + ? !`. Eleven companion Recommendations, including Update, Federated Query, Protocol and Graph Store HTTP. | [SPARQL 1.1 Query](https://www.w3.org/TR/sparql11-query/) |
| JSON-LD 1.1 | Recommendation of 16 Jul 2020: Linked Data in JSON. A context may be given by IRI, so a processor may fetch it. The security section was not in the text I could read. | [JSON-LD 1.1](https://www.w3.org/TR/json-ld11/) |
| SKOS | Recommendation of 18 Aug 2009 for thesauri and taxonomies. Concepts, schemes, `broader`/`narrower` (not transitive by convention), `related`, one `prefLabel` per language. | [SKOS Reference](https://www.w3.org/TR/skos-reference/) |
| Turtle | Recommendation of 25 Feb 2014, UTF-8, `text/turtle`. | [Turtle](https://www.w3.org/TR/turtle/) |
| N-Triples | Line-based subset of Turtle. The 1.2 Working Draft (22 Nov 2026) defines a canonical form: one triple per line, LF endings, uppercase hex, lowercase language tags. | [RDF 1.2 N-Triples](https://www.w3.org/TR/rdf12-n-triples/) |
| N-Quads | Not read for this note. RDFC-1.0 outputs canonical N-Quads, per the next row. | [RDFC-1.0](https://www.w3.org/TR/rdf-canon/) |
| RDFC-1.0 | Recommendation of 21 May 2024. Gives blank nodes deterministic identifiers so datasets can be compared, signed or hashed. SHA-256 default; SHA-384 must be supported. "Poisoned" datasets can run impractically long, so implementations must stop early. | [RDFC-1.0](https://www.w3.org/TR/rdf-canon/) |
| PROV-O | Recommendation of 30 Apr 2013. Entity, Activity, Agent; `wasGeneratedBy`, `used`, `wasDerivedFrom`, `wasAttributedTo`. | [PROV-O](https://www.w3.org/TR/prov-o/) |
| ISO GQL | ISO/IEC 39075:2024, published April 2024, 610 pages. Covers creating, querying, maintaining and controlling property graphs. | [ISO 39075](https://www.iso.org/standard/76120.html) |
| SQL/PGQ | ISO/IEC 9075-16:2023, published June 2023. Defines how SQL represents and interacts with property graphs. | [ISO 9075-16](https://www.iso.org/standard/79473.html) |
| openCypher | Resources: reference, TCK (Cucumber tests), grammar (ISO WG3 BNF in release 2024.2). openCypher 9 was "the last release ... before ... the road to GQL." Neo4j, Inc. copyright; no license text on the page. | [openCypher resources](https://opencypher.org/resources/) |

Inference: Two families need two models. RDF is a set of triples with global IRIs and a standard validation (SHACL), reasoning (OWL) and signing (RDFC) story. Property graphs (GQL, SQL/PGQ, Cypher) are queried, not validated by a shared standard I found. An Oska library should pick RDF as the interchange model and treat property-graph query as a query-language adapter over `std.sql` or a server.

Open question: I did not read the GQL or SQL/PGQ text itself (ISO pages show only abstracts), the SPARQL Update or Protocol specs, or the JSON-LD security section. Any claim about those is not made here.

## 2. Building

### Ingest

- Standards: Turtle and N-Triples are text formats ([Turtle](https://www.w3.org/TR/turtle/), [N-Triples](https://www.w3.org/TR/rdf12-n-triples/)); JSON-LD loads contexts by IRI ([JSON-LD](https://www.w3.org/TR/json-ld11/)).
- Oska today: text and bytes read through a capability are untrusted values, and converting one to a validated type is the only way to a trusted value ([GLOSSARY](../../GLOSSARY.md), "Untrusted value", "Validator").
- Inference: A parser that takes `Untrusted` text and returns a typed `Triple` (with `Iri` as a validated type) is the same shape as `std.json`. Strict N-Triples first: it is line-based, so a limit per line and per file is easy to enforce.
- Inference: Fetching a remote JSON-LD `@context` is a network effect. In Oska it would need a `HostConnect` capability ([GLOSSARY](../../GLOSSARY.md), "Host capability"), so a JSON-LD processor would be offline by default. That removes a known risk class by construction. Whether the JSON-LD spec lists this as a risk I could not confirm.

### Extraction from text with LLMs

- Oska today: `std.llm` takes a `Budget` capability that caps tokens and calls ([GLOSSARY](../../GLOSSARY.md), "Budget"). Untrusted text stays untrusted through text operations. Sink literals keep untrusted text out of structural positions ([GLOSSARY](../../GLOSSARY.md), "Sink literal", "Structural position").
- Inference: Model output built from a document is untrusted data. The safe flow is: LLM returns candidate triples as text, the program parses them with the strict codec, and only IRIs that pass a validator (known namespace, allowed vocabulary) become trusted. Triples that fail go to a review queue, not the graph.
- Inference: Record each extraction as PROV-O: the Activity is the LLM call, the Agent is the model, the Entity used is the source document hash. This is a template of about 40 lines over the vocabulary in [PROV-O](https://www.w3.org/TR/prov-o/).

### Entity resolution

- Standards: SKOS has `exactMatch`/`closeMatch` mapping links ([SKOS](https://www.w3.org/TR/skos-reference/); the section was truncated in my read, so details are not claimed).
- Inference: Resolution should emit candidate match triples with a score and a provenance, not merge nodes. `Map` with insertion order gives deterministic blocking and output ([GLOSSARY](../../GLOSSARY.md), "Insertion order"). Merging is a human or approver step; the `Approval` value is single use and shown with its summary ([GLOSSARY](../../GLOSSARY.md), "Approval").

### Validation and provenance

See sections 3 and 4 for SHACL and RDFC.

## 3. Operating

### Storage and query

- Oska today: `std.sql` with `sql"..."` sink literals, where text is structural and parameters are data ([GLOSSARY](../../GLOSSARY.md), "Sink literal", "Data position"). Per its glossary entry, `Map` has an entry limit.
- Inference: Smallest durable store: a SQLite table of quads `(g, s, p, o)` with indices, written through `std.sql`. SQL/PGQ ([ISO 9075-16](https://www.iso.org/standard/79473.html)) is the standard bridge from SQL to graphs, but I would not target it before the engine supports it; I did not verify any engine does.
- Inference: A SPARQL client over `std.http` (SPARQL Protocol, [SPARQL 1.1 suite](https://www.w3.org/TR/sparql11-query/)) costs far less than an engine: build the query from a sink literal, send with a `HostConnect` to one endpoint. A query string with untrusted text in a structural position would be a compile error, which is the SPARQL-injection answer.

### Reasoning

- Standards: OWL 2 RL is polynomial and aimed at rule engines over RDF triples; EL is polynomial; QL rewrites to relational queries ([OWL 2 Overview](https://www.w3.org/TR/owl2-overview/)). RDFS inference is limited ([RDFS](https://www.w3.org/TR/rdf-schema/)).
- Inference: If anything, implement RDFS subclass/subproperty closure and a few OWL 2 RL rules, run with a step limit (Oska already has one, [ADR 0021](../adr/0021-a-step-limit-stops-at-the-same-step-on-every-backend.md)) so a hostile ontology cannot loop. Full OWL DL is not in scope.

### Versioning, diff, patch, and access control

- Standards: RDFC-1.0 yields canonical N-Quads, so a hash of a graph is stable across serializations ([RDFC-1.0](https://www.w3.org/TR/rdf-canon/)). N-Triples canonical form gives one line per triple ([N-Triples](https://www.w3.org/TR/rdf12-n-triples/)).
- Oska today: Bytes and crypto in `std` (the glossary says TLS, git and crypto are written in Oska, [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)); the lockfile records content hashes for packages ([GLOSSARY](../../GLOSSARY.md), "Packages"). I did not verify a general content-addressed store or build cache API for user data.
- Inference: Diff of two graphs without blank nodes is a set difference of sorted canonical N-Triples lines: tiny. With blank nodes, canonicalize first and bound the work, because RDFC says poisoned datasets must be aborted. A patch is a list of added and removed triples; a graph version is a hash of its canonical form plus a signed parent hash.
- Inference: Access control maps to capabilities: a `GraphRead` capability attenuated to one named graph, as `DirRead` is attenuated to a directory ([GLOSSARY](../../GLOSSARY.md), "Attenuation"). Classification ([ADR 0029](../adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md), decided, not built) could tag triples that came from restricted documents so they cannot reach an `std.llm` sink above its ceiling.

## 4. Maintenance

### Constraint validation and schema evolution

- Standards: SHACL validates a data graph against shapes and returns an RDF report ([SHACL](https://www.w3.org/TR/shacl/)). SKOS integrity conditions exist, for example one `prefLabel` per language ([SKOS](https://www.w3.org/TR/skos-reference/)).
- Inference: SHACL-core (class, datatype, cardinality, node kind, pattern, in) is a small subset that covers most checks. Contracts in signatures could state "takes a graph that conforms to shapes S" as a validated type: the validator returns `Conforming<Graph>` and downstream code relies on it, the same way `Email` works.
- Inference: Schema evolution is a diff of two shapes graphs and ontologies, using the same diff tool. A release check could refuse a change that removes a class still used, like the widening gate for authority ([GLOSSARY](../../GLOSSARY.md), "Widening").

### Drift and audit

- Inference: Drift (extraction output changing as the model changes) is measurable: re-run extraction on a fixed corpus with a pinned model, canonicalize, and diff against the last accepted graph. Oska's pinned float results and ordered `Map` ([ADR 0027](../adr/0027-float-results-are-pinned-by-the-spec-on-every-platform.md), [ADR 0028](../adr/0028-a-map-iterates-in-insertion-order-with-a-fixed-unseeded-hash.md)) make the non-model parts of the pipeline reproducible. The model itself is not.
- Inference: Audit is PROV-O plus signed graph hashes plus the audit-site list for trust assertions ([GLOSSARY](../../GLOSSARY.md), "Audit site").

## 5. Ranked additions

All sizes and rankings are Inference.

| # | Addition | Rough size | Why first |
|---|---|---|---|
| 1 | `std.rdf`: `Iri`, `Literal`, `Triple`, `Graph` (ordered set), strict N-Triples and N-Quads reader and writer in canonical form, with limits | 600 to 900 lines | Everything else needs a value type and a safe parser; mirrors `std.json` |
| 2 | `std.rdf.diff`: set diff and patch of canonical triple lines | 150 lines | Needs only #1 |
| 3 | `std.shacl` core subset returning a report as a graph | 800 to 1,200 lines | Turns validation into a typed gate |
| 4 | RDFC-1.0 canonicalization and hash with a work limit, on `Bytes` and SHA-256 | 400 to 600 lines | Signing and versioning; the spec demands the limit |
| 5 | Triple-store on `std.sql` (quads, indices, graph-scoped capability) | 300 lines | Durable operation without an engine |
| 6 | SPARQL client over `std.http` with sink-literal queries and result parsing | 400 lines | Reaches existing stores |
| 7 | PROV-O helper and an `std.llm` extraction template with validator and review queue | 300 lines | Makes LLM extraction auditable |
| 8 | Turtle and JSON-LD readers (offline contexts only) | 1,000+ lines each | Large grammars; do after the rest |
| 9 | RDFS and OWL 2 RL rule subset | 800+ lines | Only on demand |

Language additions, smaller still: none are required. Inference: a validated-type pattern for `Iri` and `Conforming<Graph>` works with what the glossary says exists. A built-in `iri"..."` sink-literal would be convenient but is optional.

## 6. Risks and what not to build now

- Inference: Do not build a full OWL 2 DL or Full reasoner. OWL 2 DL maps to SROIQ, and the overview states profiles exist because they have lower complexity ([OWL 2 Overview](https://www.w3.org/TR/owl2-overview/)). It is a research project, and existing engines do it.
- Inference: Do not build a SPARQL engine or a native graph database yet. Use a client and `std.sql`.
- Inference: Do not target RDF 1.2 triple terms yet. The spec is a draft ([RDF 1.2](https://www.w3.org/TR/rdf12-concepts/)); N-Triples 1.2 is a Working Draft that "may be updated, replaced, or obsoleted" ([N-Triples 1.2](https://www.w3.org/TR/rdf12-n-triples/)). Ship RDF 1.1.
- Inference: Do not implement GQL. It is 610 pages ([ISO](https://www.iso.org/standard/76120.html)). openCypher 9 is no longer the active line ([openCypher](https://opencypher.org/resources/)) and its page gives no license text.
- Risk, canonicalization: poisoned graphs can burn CPU ([RDFC-1.0](https://www.w3.org/TR/rdf-canon/)). Always set a work limit.
- Risk, LLM extraction: a model can invent IRIs and facts. Trust-tracked text does not make a claim true; it only stops the claim from becoming code or a structural query. A validator decides what counts as trusted, and that validator is the weak point.
- Risk, classification is decided but not built ([ADR 0029](../adr/0029-classification-is-a-flag-on-text-and-bytes-with-sink-ceilings.md)), so per-triple classification waits on it.
- Not verified: the state of `std.sql`, `std.http`, and `std.llm` in code. I read only the glossary and ADR titles, so every "Oska today" claim above is from those docs.

## Sources

1. https://www.w3.org/TR/rdf11-concepts/
2. https://www.w3.org/TR/rdf12-concepts/
3. https://www.w3.org/TR/rdf-schema/
4. https://www.w3.org/TR/owl2-overview/
5. https://www.w3.org/TR/shacl/
6. https://www.w3.org/TR/sparql11-query/
7. https://www.w3.org/TR/json-ld11/
8. https://www.w3.org/TR/skos-reference/
9. https://www.w3.org/TR/turtle/
10. https://www.w3.org/TR/rdf12-n-triples/
11. https://www.w3.org/TR/rdf-canon/
12. https://www.w3.org/TR/prov-o/
13. https://www.iso.org/standard/76120.html
14. https://www.iso.org/standard/79473.html
15. https://opencypher.org/resources/
16. [GLOSSARY.md](../../GLOSSARY.md) and the ADR titles in [docs/adr/](../adr/) (repository)
