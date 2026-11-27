# How could Oska have its own AI-first, secure-by-default data stores and databases?

Date of access: 2026-11-27 (all external pages read that day). Oska facts were checked against the repo at `/home/user/oska` the same day. "Inference:" marks my own conclusions; everything else is a cited fact.

## Summary

- Inference: Oska should not start with a SQL server. The smallest useful step is an **embedded, single-writer, append-only store** (a hash-chained record log plus a snapshot) that lives inside one `DirWrite`, is opened by a `Store` capability, and is written in Oska. It is typed, not string-queried, and it carries trust and classification labels in every record.
- Inference: Later steps, in order: encryption at rest keyed from a capability; a Merkle-tree audit log; secondary indexes and full-text search; vector search; an MCP tool facade over a narrowed store capability; time travel and deterministic replay; a TLA+ crash-safety model written before the engine.
- Inference: The main reason to build rather than bind SQLite is that Oska's guarantees (trust flag, classification ceiling, capability narrowing, audit sites) stop at the library boundary of any foreign engine. A native store can carry them into the bytes on disk.

### Recommended architecture

Smallest first step (v0):

1. `std.store`: `Store<T>` is a capability-bearing struct holding a `DirWrite`/`DirRead` (ADR 0008) for one directory. Records are typed Oska structs with a derived schema. Inference: reuse the `json.Schema` machinery in `crates/oska-check/src/std/json.osk` so the schema doubles as the MCP input schema.
2. Storage: one append-only segment file of length-prefixed, checksummed records, each carrying the SHA-256 of the previous record (hash chain), plus a periodic snapshot file written to a temporary name and renamed. In-memory index is a `Map` (insertion-ordered, fixed unseeded hash, ADR 0028), rebuilt from the snapshot and the log tail.
3. One writer, many readers. No network, no server. A server is a separate program that holds a `Store` and a `NetListen`-style capability.
4. Every record has a label header: trust bit (was it validated), classification (ADR 0029), writer identity, and a monotonically increasing sequence number.
5. Queries are typed method calls (`store.where(Field.eq(...))`), not strings. `std.sql` stays for talking to external databases.

Later:

- v1: encryption at rest (AEAD per record, key from HKDF), signed checkpoints (Merkle tree head), `Store.read_only()` attenuation.
- v2: secondary indexes (B-tree pages in the file) and an inverted index for full text.
- v3: vector index (HNSW), agent-memory schema, MCP facade.
- v4: time travel by sequence number, deterministic replay of a log against a model.
- v5 (only if needed): LSM or B-tree page engine behind the same interface; a SQL front end over the typed engine; a network server.

## What exists in Oska today, and what does not

| Item | State | Evidence |
| --- | --- | --- |
| `std.sql` | Checks, has **no runtime**: `oskac run` refuses a program that imports it (`E0371`). Query text must be a `sql"..."` literal (`SqlLiteral`, private field); parameters are `List<Untrusted<String>>`; rows come back untrusted. `Db` wraps a `NetConnect`, so it reaches a database over the network only. | `crates/oska-check/src/std/sql.osk`; `E0371` in `crates/oska-run/src/panics.rs` |
| `std.json` | Implemented in Oska, 1123 lines. Parsed strings, keys included, are untrusted; numbers and booleans trusted. Has `json.Schema`. | `crates/oska-check/src/std/json.osk` |
| `std.mcp` | Exists: `Toolset` trait (`specs`, `call`), `ToolSpec` with a `json.Schema` input, `ToolResult`, `serve` over stdio. A tool reaches only what its toolset holds. | `crates/oska-check/src/std/mcp.osk`; GLOSSARY "Toolset" entries |
| Crypto in Oska | SHA-2, HMAC, HKDF (on `Secret<Bytes>`), ChaCha20-Poly1305, AES-128-GCM, Ed25519, ECDSA P-256, X25519, RSA, SSH signatures, X.509, TLS 1.3 client. No AES-GCM-SIV, no XChaCha20, no SHA-3/BLAKE, no password hash (Argon2/scrypt). There is no module literally named `std.crypto`; the primitives are separate modules. | `crates/oska-check/src/std/{sha2,hmac,hkdf,chacha20poly1305,aes128gcm,ed25519,ecdsa,p256,x25519}.osk`; ADR 0012 |
| `Bytes`, secret integers | Prelude buffer with trust flag; `Secret<U8/U32/U64>` constant-time (native code only). `expose()` is an audit site. | GLOSSARY "Bytes", "Secret integer"; ADR 0012 |
| `Map<K,V>` | Prelude, insertion-ordered, fixed unseeded hash, refuses `Float` and secret keys; has an entry limit. | ADR 0028; `crates/oska-check/src/std/map.osk` |
| File access | Only confined capabilities. `DirRead`: `read_to_string`, `read_bytes`, `list_dir`. `DirWrite`: `write`, `write_bytes`, `create_dir`. `FileRead`/`FileWrite`: whole-file read/write. | ADR 0008; `crates/oska-check/src/prelude.osk` |
| **Missing for a store** | No append, no positional read/write, no `fsync`, no atomic rename, no file lock, no `mmap` exposed, no delete in the prelude file API. The only `fsync` (`sync_all`) in the repo is in the Rust snapshot-test helper. | grep of `prelude.osk`; `crates/oska-run/src/snapshots.rs:425` |
| Classification (ADR 0029) | **Decided, not built.** Four levels `Public < Internal < Confidential < Restricted`; flag on text and bytes; sink ceilings; `Declassify` capability. "Erased after checking, so no runtime flag is expected." Parsing and `std.sql` carry-through follow the first spec. | `docs/adr/0029-*.md`; GLOSSARY "Classification" |
| Trust | Flag on text and bytes; validators produce trusted typed values; sink positions split structural vs data. | ADR 0006; GLOSSARY "Untrusted value", "Validator", "Sink literal" |
| Network | Host-narrowed capabilities; everything read from a connection is untrusted. | ADR 0025 |
| Concurrency | Tasks, scopes, channels, model-checked in TLA+ (ADR 0019). Models cover scheduler, reference counts, channels. No storage or crash model. | `formal/tla/*.tla`, `formal/tla/parallel-tasks.md` |
| Lean | Reference model of Oska Core, ADR 0018. | `formal/lean/`, ADR 0018 |
| Database engine, vector search, full-text search, encryption at rest, audit log | **None exist.** | no matching source in `crates/` |

Inference: the prelude has no durable-write primitives, so the first work item is not the store but the file-API additions (append, `sync`, atomic rename, exclusive lock) as new capability methods, designed under ADR 0008's rule that every method is confined and attenuating.

## Embedded versus server

- SQLite positions itself as competing with `fopen()`, for device-local data with low write concurrency, and recommends client/server when the data is across a network, writers are many, or data nears a terabyte ([SQLite, Appropriate Uses](https://www.sqlite.org/whentouse.html)).
- SQLite WAL: readers do not block the writer, still one writer; all processes must be on the same host, and WAL does not work over network filesystems ([SQLite WAL](https://www.sqlite.org/wal.html)).
- Inference: An embedded store fits Oska's main use (an agent program or a tool with its own data). An embedded store also keeps the capability story simple: the store's authority is exactly the `DirWrite` it was given, visible in `main`'s signature and the authority report.
- Inference: A server adds a listener, authentication, multi-tenant policy, and a wire protocol, each a new attack surface. Defer it. When it comes, make it an ordinary Oska program that holds a `Store` and serves MCP or a typed protocol; do not put a server inside the engine.

## Storage engine choices

| Option | Facts | Inference for Oska |
| --- | --- | --- |
| Single-file B-tree (SQLite) | Fixed-size pages; table and index B-trees; page 1 holds the schema; a rollback journal or WAL supports crash recovery ([SQLite file format](https://www.sqlite.org/fileformat2.html)). | Best read and range performance, most complex. Needs positional I/O, page cache, locking and a free list, none of which exist yet. Later. |
| LSM tree (RocksDB) | Memtable, optional WAL, immutable sorted SST files with checksummed blocks, background compaction; a tradeoff among write, space and read amplification ([RocksDB overview](https://github.com/facebook/rocksdb/wiki/RocksDB-Overview)). | Immutable files fit a hash chain and Merkle tree well, but compaction needs background tasks and tuning. Later, if write volume demands. |
| Append-only log plus snapshot | No external source read; the technique is generic. | Smallest, easiest to verify, and the natural carrier for a hash chain. Memory-bound index, so suited to data in the megabytes to low gigabytes (agent memory, config, audit). Start here. |
| Copy-on-write B+tree (LMDB, BoltDB) | I could not read either project's design notes (LMDB's site blocked automated access; the BoltDB fetch was rate limited). Not cited. | Worth a second look: copy-on-write gives snapshots and crash safety without a WAL, but it needs `mmap`, which Oska does not expose. |

Inference: the log-plus-snapshot design gives time travel and tamper evidence almost for free, because the log is the database and old states are prefixes of it.

## Encryption at rest and key handling with capabilities

- Facts: AES-GCM-SIV is nonce-misuse resistant: a repeated nonce leaks only whether two plaintexts were equal, and it derives per-nonce keys ([RFC 8452](https://www.rfc-editor.org/rfc/rfc8452)). Oska has ChaCha20-Poly1305 and AES-128-GCM (`crates/oska-check/src/std/chacha20poly1305.osk`, `aes128gcm.osk`), not GCM-SIV.
- Inference: Seal each record separately with ChaCha20-Poly1305 under a per-record key from HKDF (`hkdf.osk`, which takes `Secret<Bytes>`) over a master key and the record's sequence number. A sequence number is unique by construction, so no random nonce is needed and a misuse-resistant mode is not required. Put the record header (sequence number, labels, previous hash) in the AAD so labels cannot be edited without detection.
- Inference: The master key is a `Secret<Bytes>` held in a `StoreKey` capability. `StoreKey` cannot be formatted, logged or compared with `==` (GLOSSARY "Secret value"). The only way out is `expose()`, an audit site. `Store.open(dir, key)` takes both, so "who can read this database" equals "who holds the `DirRead` and the `StoreKey`", and the authority report shows both.
- Inference: Key rotation is a new snapshot sealed under a new key, with the log tail re-sealed. A narrowed `StoreKey` can be a derived subkey per table or per classification level, so an agent holding the `Internal` subkey cannot decrypt `Restricted` records even with file access.
- Inference: A password-derived key needs a memory-hard hash, which is missing. Start with a key supplied by the caller (a capability), not a passphrase.
- Caveat from the repo: constant time holds for native code only, and classification gives no timing guarantee (ADR 0029, GLOSSARY "Constant time").

## Labels carried into storage

- Fact: Postgres row security is default deny once enabled, can be bypassed by superusers and `BYPASSRLS` roles, and referential-integrity checks bypass it, which can leak through covert channels ([PostgreSQL row security](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)).
- Inference: In Postgres a policy is a runtime rule written by a human administrator. In Oska the equivalent can be a type: `Store<T>` records carry a classification and trust bit, and a read returns `Confidential` (or `Untrusted<...>`) values that the checker already refuses to send to a lower-ceiling sink. The leak the Postgres page describes (a side channel in constraint checks) is smaller when the engine has no superuser, because authority is a capability, not a role.
- Inference: ADR 0029 erases classification after checking, so a stored label must be written explicitly: the store records the label as data, and `open` re-attaches it when reading. The re-attachment must be the only place a value gains a classification from a stored byte, and the stored label must sit inside the AEAD associated data so a file editor cannot downgrade it. This is a new rule that ADR 0029's "reading from a capability may give the capability's own classification" (still unsettled) must decide.
- Inference: Per-column classification is declared in the schema; per-row classification is the join of its columns (ADR 0029: operations give the highest classification of inputs). Writing a value whose classification is above the column's declared level is a compile error; lowering needs `Declassify`.
- Inference: Trust at rest: a stored value written from untrusted text is returned as untrusted; a stored validated type (an `Email`) is returned trusted only if the schema's validator is rerun on read, or the record is signed by a writer that held the validator. Otherwise a corrupted file would manufacture trusted values.
- Inference: Row-level tenancy is a capability, not a predicate: `store.scope(Tenant("a"))` returns a narrowed store that cannot name other tenants' keys. This replaces a policy language with attenuation, the same pattern as `restrict_to`.

## Tamper-evident audit log

- Facts: RFC 9162 defines a Merkle tree with domain-separated leaf and node hashes (`0x00`, `0x01`), inclusion proofs, consistency proofs (an older tree is a prefix of a newer one), and a signed tree head ([RFC 9162](https://www.rfc-editor.org/rfc/rfc9162)).
- Inference: Every `Store` write appends a leaf. A signed tree head (Ed25519, `ed25519.osk`) is published periodically. Anyone holding an old head can verify with a consistency proof that history was only appended. All needed primitives (SHA-256, Ed25519) already exist in Oska.
- Inference: The audit log records which capability wrote what, which MCP tool was called, and by which agent session. Because the program passes capabilities explicitly, the writer identity is a value in scope, not an inferred login.
- Inference: A hash chain alone detects edits but not truncation of the tail; the signed head, kept elsewhere (another capability, such as a `NetConnect` to a witness), closes that.

## A schema and query language an LLM writes safely

- Facts: `std.sql` already makes query text a `sql"..."` literal and parameters untrusted data (`crates/oska-check/src/std/sql.osk`). The earlier note in the repo makes the same point for SQL injection: text outside literals cannot be structural (`docs/research/2026-11-ai-first-secure-language.md`, finding 6).
- Inference: For the native store the query is a typed value, not text. `Query<T>` is built from field accessors derived from the schema; there is no string to inject into, so the sink-literal rule becomes unnecessary rather than enforced. This also removes the second hazard LLMs have with SQL: wrong column names fail to compile with a "did you mean" diagnostic, which is a better feedback loop than a runtime SQL error.
- Inference: Keep a SQL front end as an optional later layer: the `sql"..."` literal is parsed at compile time against the schema, so the checker can reject unknown columns and type-mismatched parameters before the program runs. Oska has no user-defined macros (ADR 0003), so that parsing must be built in the compiler, not as a library.
- Inference: Migrations are values: `Migration(from: 3, to: 4, fn)` with the old and new schema types both in scope, so a migration that drops a classified column without `Declassify` fails to check.
- Inference: Resource limits are part of the API: every query takes a row budget and a step budget in the same style as the existing step limit (ADR 0021) and token budget (GLOSSARY, `std.llm` budget), so a model cannot write an unbounded scan.

## Vector and full-text search for AI

- Facts: HNSW is a multilayer proximity graph with logarithmic scaling claims by its authors ([Malkov and Yashunin, arXiv:1603.09320](https://arxiv.org/abs/1603.09320)). pgvector defaults to exact search; approximate indexes (HNSW, IVFFlat) trade recall for speed, and filters are applied after the index scan, so a filtered query can return fewer rows than asked ([pgvector README](https://github.com/pgvector/pgvector/blob/master/README.md)).
- Inference: Start with **exact** search (a flat scan over `F32` vectors in a `Bytes` buffer). It is simple, has perfect recall, and is fast enough for tens of thousands of memories. Add HNSW only when measured to be needed. This requires floats (ADR 0027 pins float results; `crates/oska-run/src/float.rs` exists, but whether `Float` is complete is not verified here).
- Inference: Apply the label filter **before** the nearest-neighbour step, not after. pgvector's post-filtering is a correctness hazard for security: a post-filter that drops rows the caller cannot see still lets the index traversal touch them, and returns a short result. In a capability-narrowed store the narrowed view simply does not contain the other records, so the index is per-view.
- Inference: Embeddings are classified like the text they came from. An embedding of `Restricted` text is `Restricted`; embeddings can leak their source, so the model call that makes one needs `std.llm` budget and a sink ceiling at least as high.
- Inference: Full text: an inverted index over tokens held in the same log, with BM25 ranking. Straightforward in pure Oska; no new language feature.
- Inference: Hybrid retrieval (keyword plus vector, fused by rank) as one typed call returning `List<Hit<T>>` with a score and the record's labels.

## Agent memory

- Inference: A built-in `Memory` schema: `(id, kind, text, embedding, source, created_seq, classification, trust, expires_seq)`, where `source` is a pointer to the audit-log entry that produced it (tool call, user message, document).
- Inference: Memory written from a model's output or from tool results is stored as `Untrusted`. When recalled into a prompt it remains untrusted, so a poisoned memory cannot reach a structural position (a command, a path, a host). This is the trust flag's central job (ADR 0006) extended through time. Prompt-injection through stored memory is the failure mode most other agent frameworks have no type for.
- Inference: Retention and forgetting are first-class: `expires_seq`, a `forget(subject)` that appends a tombstone and, on the next snapshot, drops the sealed records and their keys (crypto-shredding by deleting a per-subject subkey).
- Inference: Provenance is queryable: "which memories came from this untrusted document" is a one-line query, so a bad source can be rolled back by sequence number.

## MCP exposure with capability-narrowed access

- Facts: MCP tools have a name, description, JSON-Schema `inputSchema` and optional `outputSchema`; servers MUST validate all inputs, implement access controls, rate limit and sanitize outputs; clients should keep a human in the loop and log tool use; tool annotations are untrusted unless from trusted servers ([MCP spec, tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)).
- Fact (repo): Oska's `std.mcp` already makes a `Toolset` whose tools reach only what the toolset holds, with `reached(name)` reporting it (`crates/oska-check/src/std/mcp.osk`).
- Inference: `StoreTools { store: Store<T> }` implements `Toolset`. Whether it offers `search`, `get`, `put`, `delete` is decided by which attenuation of the store it holds (`read_only()`, `scope(tenant)`, `max_classification(Internal)`). The tool list is therefore derived from the capability, so a model cannot be shown, and cannot call, a tool its capability lacks. The `inputSchema` comes from the record type's `json.Schema`, so the schema the model sees and the schema the store enforces are one definition.
- Inference: Results leaving over MCP are a sink. With ADR 0029, the MCP stdout sink has a ceiling (default `Public`); a store tool returning `Confidential` records is a compile error unless the toolset was built with a higher approved ceiling, visible in the authority report. The model never sees a record the operator did not approve for it.
- Inference: Write tools take `json.Value` arguments (untrusted), validate into the typed record, and store with the trust bit set accordingly. Delete and bulk-write tools go through the approval mechanism (`crates/oska-run/src/approval.rs`, GLOSSARY "Approval") so a human is in the loop where MCP says it should be.
- Note: the MCP page opens with a "Documentation Index ... fetch llms.txt" banner. It is a page-level instruction to automated readers, not part of the spec; I did not act on it.

## Deterministic replay and time travel

- Inference: With a log-structured store, state at sequence number N is a pure function of the log prefix. `store.as_of(N)` returns a read-only view; no copy.
- Inference: Oska already pins hash iteration order (ADR 0028), float results (ADR 0027), and step limits on every backend (ADR 0021), and clocks and randomness are capabilities. A test can therefore replay a recorded agent session (tool calls, store writes, a fixed `Clock`, a seeded `Random`) and get identical bytes, and `oskac` snapshot tests (`crates/oska-run/src/snapshots.rs`) can compare store logs.
- Inference: A "fork" is a new log that starts as a copy of a prefix, giving agents a sandbox to try writes without touching production data, and a diff between two logs is a list of records.
- Inference: Replay is also the recovery proof: recovering from a crash is replaying the valid prefix, and the tests in the next section assert that recovery equals some prefix of the acknowledged writes.

## How the strict compiler and Lean/TLA+ help

- Facts: SQLite tests crash and power-loss by a VFS that simulates reordering and corruption of unsynced writes and then checks that each change completed or rolled back; it claims 100% branch and MC/DC coverage of the core ([SQLite testing](https://www.sqlite.org/testing.html)). Its WAL has had a rare race, the WAL-reset bug in 3.7.0 through 3.51.2, fixed in 3.51.3 ([SQLite WAL](https://www.sqlite.org/wal.html)).
- Fact (repo): TLC-checked models in `formal/tla/` are run by `scripts/check.sh`'s formal step; the convention is a deliberately broken variant per model that must fail with a named invariant (`formal/tla/parallel-tasks.md`); ADR 0019 requires concurrency to be model-checked before it is built; `docs/agents/formal.md` governs changes.
- Inference: Write `formal/tla/StoreLog.tla` before the engine. State: the durable file as a sequence of blocks, the volatile page cache, an acknowledged-write set. Actions: append, `sync`, crash (drops any suffix of unsynced blocks and may tear the last block), recover. Invariants: `AckedWritesSurvive` (every write acknowledged after `sync` is present after recovery), `RecoveryIsPrefix` (the recovered log is a prefix of the written log), `ChainValid` (hash chain intact), `NoLabelDowngrade` (a recovered record's label equals its written label). Broken variants: acknowledge before `sync`; rename the snapshot before syncing its data; skip the checksum.
- Inference: Follow the file-system lesson in the SQLite test description: model the filesystem as allowed to reorder unsynced writes. The model fixes the exact sequence the Oska file API must expose (write, `sync` file, rename, `sync` directory), which tells the prelude work what primitives to add.
- Inference: A fault-injection `Fs` implementation for tests, in the same family as the existing in-memory test `fs` (`crates/oska-check/src/prelude.osk`, `Testing` stand-ins), lets the engine's own tests crash at every write boundary, reproducing the SQLite approach inside `oskac test`.
- Inference: Lean: the flow rule and ceiling check are in scope of ADR 0029's Lean work; extend the same model with "stored label is restored exactly" as a lemma over a toy store. A noninterference proof for the store is out of reach near-term, as ADR 0029 already says for the language.
- Inference: The strict compiler helps in specific, checkable ways: exhaustiveness (`usefulness.rs`) makes recovery code handle every record kind; `E`-level unused-authority errors (ADR 0016) flag a store opened with write authority that only reads; the authority diff (`crates/oska-check/src/authority_diff.rs`) shows when a change widens a store's classification ceiling; no macros and enforced capitalization (ADRs 0003, 0005) keep generated code uniform for models to read.

## What "AI-first" means concretely

1. Schema is the tool definition: one record type yields the storage format, the validator, the `json.Schema`, and the MCP tool spec.
2. No query strings: typed query values; unknown fields fail at check time with an actionable diagnostic.
3. Injection is impossible by construction: untrusted text cannot reach a structural position, and stored memory stays untrusted when recalled.
4. Authority is narrowable to the task: read-only, per-tenant, per-classification views, handed to an agent as a value; the tool list follows.
5. Every call is bounded: row, step and token budgets are parameters, not conventions.
6. Built-in retrieval: exact vector, full-text and hybrid search, with label filtering done before ranking.
7. Memory with provenance, expiry and forgetting, rollbackable by source.
8. Everything is attributable: tamper-evident log of which capability and which tool call wrote each record.
9. Reproducible: time travel and replay with fixed clock, seed and iteration order.
10. Human review is cheap: the authority report lists every store, key, ceiling and `Declassify` holder (ADR 0017 self-assessment).

## Oska's unique opportunities

- Labels survive in the type system and can be made to survive on disk. Other embedded databases (SQLite, LMDB, RocksDB) treat values as opaque bytes; the label and the ceiling check cannot cross their API.
- Capabilities replace roles and policies. Attenuation (`read_only`, `scope`, `max_classification`) is the access-control language, with no separate DSL to get wrong, and no superuser (compare Postgres's `BYPASSRLS` and owner bypass above).
- The checker already treats SQL text as a structural sink. A native store goes one step further and removes the text.
- Crypto, hash, signature and JSON libraries are already in Oska (ADR 0012), so an encrypted, signed, Merkle-logged store has no foreign dependency to audit. The `oskac` binaries are statically buildable and deterministic across hosts (GLOSSARY "Supported platform").
- Deterministic everything (map order, float results, step limits, capability clocks) makes replay and time travel realistic, which most languages cannot promise.
- The native backend and the interpreter must agree (ADR 0014), so a store's behaviour is tested on both with the same program.
- Existing MCP `Toolset` and approval machinery mean the agent-facing surface is mostly written.
- Authority reports and `Declassify` audit sites give a compliance story (self-assessment only, ADR 0017).

## Risks and non-goals

- Risk: Durability needs file primitives the prelude lacks (see table). Without `fsync` and atomic rename, claims about crash safety would be false. Do not ship a store before the API and the TLA+ model agree.
- Risk: Building an engine is large. SQLite's test regime (100% MC/DC, fault injection) is the bar for trust; a small log-structured store is chosen because it is verifiable at that size, not because it is faster.
- Risk: Classification is erased after checking (ADR 0029); the stored-label design is new and has not been through the grilling that ADR 0029 got. Tampered or stale labels are the main attack surface.
- Risk: Implicit flows stay open (ADR 0029): code branching on a classified value's length can leak. The store inherits this.
- Risk: Constant-time guarantees hold only for native code; interpreted runs of the store can leak through timing.
- Risk: Memory limit of `Map` and an in-memory index cap the data size; large datasets need the page-based engine.
- Risk: Embedding vectors need floats; `Float` completeness was not verified for this note.
- Non-goals for v0: SQL server, wire protocol, replication, multi-writer, distributed transactions, a full SQL dialect, ISO SQL conformance, user-defined functions, network filesystem support.
- Non-goal: claiming compliance (FIPS, ISO 27001) from labels or encryption alone (ADR 0017, ADR 0029; `docs/research/2026-11-fips-140-3.md` exists but I did not open it).

## Open questions

1. Which primitives enter the prelude first, and under which capability: `DirWrite.append`, `FileWrite.sync`, `DirWrite.rename`, an exclusive-lock capability? ADR 0008 forces each to be confined.
2. Does reading a stored value restore its classification from the file, or from the capability (ADR 0029's unsettled point)? I recommend both: the max of the stored label and the capability's own.
3. Is a stored `Public` label ever trusted without `Declassify`? Inference: no; only `Declassify` lowers.
4. One `Store<T>` per record type, or one store with several tables sharing a log and a Merkle tree?
5. Where does the signed tree head live, and which capability publishes it?
6. Is a memory-hard password KDF (Argon2) worth adding to `std`, or should keys always be caller-supplied?
7. Should the typed query layer also compile a restricted SQL subset, given that Oska has no user macros (ADR 0003)?
8. Does the approval mechanism extend to store writes by agents (per-call, per-batch, per-table)?
9. How do snapshot-tests (`oskac test`) represent a store log, as text, as `Bytes`?
10. Does a CBOR or custom binary record format beat JSON for record encoding? JSON is already implemented; its parse cost and number-as-text form are a concern for speed.

## Not verified

- LMDB and BoltDB design notes: not read (robots block and rate limit). Copy-on-write B+tree claims are therefore uncited.
- SQLCipher and the XChaCha20 draft: not read. AES-GCM-SIV and the per-record key scheme come from RFC 8452 and my own design.
- Datalog and the SQL standard: not consulted; the knowledge-graph note (`docs/research/2026-11-knowledge-graphs-and-ontology.md`, "Storage and query") already proposes a SQLite quad table via `std.sql`, which has no runtime today.
- Whether `Float` is fully implemented, and whether `std.llm` exists as a module in the repo (GLOSSARY mentions its budget; I did not find `std/llm.osk` in the std listing).

## Sources

External (accessed 2026-11-27):

- SQLite, Appropriate Uses: https://www.sqlite.org/whentouse.html
- SQLite, Write-Ahead Logging: https://www.sqlite.org/wal.html
- SQLite, How SQLite Is Tested: https://www.sqlite.org/testing.html
- SQLite, Database File Format: https://www.sqlite.org/fileformat2.html
- RocksDB wiki, Overview: https://github.com/facebook/rocksdb/wiki/RocksDB-Overview
- RFC 9162, Certificate Transparency 2.0: https://www.rfc-editor.org/rfc/rfc9162
- RFC 8452, AES-GCM-SIV: https://www.rfc-editor.org/rfc/rfc8452
- PostgreSQL, Row Security Policies: https://www.postgresql.org/docs/current/ddl-rowsecurity.html
- Malkov and Yashunin, HNSW: https://arxiv.org/abs/1603.09320
- pgvector README: https://github.com/pgvector/pgvector/blob/master/README.md
- MCP specification 2025-06-18, Tools: https://modelcontextprotocol.io/specification/2025-06-18/server/tools

Repo (checked 2026-11-27): `GLOSSARY.md`; `docs/adr/0003`, `0005`, `0006`, `0008`, `0012`, `0014`, `0016`, `0017`, `0018`, `0019`, `0021`, `0025`, `0027`, `0028`, `0029`; `crates/oska-check/src/std/{sql,json,mcp,map,sha2,hmac,hkdf,chacha20poly1305,aes128gcm,ed25519}.osk`; `crates/oska-check/src/prelude.osk`; `crates/oska-run/src/{panics,snapshots,approval,float}.rs`; `formal/tla/parallel-tasks.md`; `docs/research/2026-11-ai-first-secure-language.md`; `docs/research/2026-11-knowledge-graphs-and-ontology.md`.
