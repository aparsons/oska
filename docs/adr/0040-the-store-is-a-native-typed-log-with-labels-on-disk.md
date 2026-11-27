# The store is a native, typed, hash-chained log with record labels on disk, built after its TLA+ crash model

## Decision

Oska gets its own embedded database, `std.store`, written in Oska. It is not a runtime for `std.sql` over SQLite, and it is not a server.

- **Engine.** A **store** is a hash-chained append-only log plus a snapshot, with one writer and many readers, opened with a directory capability (ADR 0008) and a **store key**. Each record is sealed on its own with an authenticated cipher, and its header holds the sequence number, the **record label** and the hash of the previous record. The header is authenticated data, so editing the file cannot change a label.
- **Labels.** The store writes the classification (ADR 0029) and the trust flag (ADR 0006) with each record. A read gives the higher of the record label and the classification of the capability that opened the store. Only a `Declassify` capability lowers it. This settles the point that ADR 0029 left to the spec.
- **Queries.** The first version takes typed queries only: values built from the record type's fields, with no query text. A checked SQL subset is a later layer and does not change the storage.
- **Order of work.** A TLA+ model of the log, sync, rename and crash comes first (ADR 0019 for concurrency, the same rule here). Its actions fix which file operations the prelude must add: append, sync, atomic rename, directory sync and an exclusive lock, each a confined capability method. The engine comes last.

## Why

SQLite, LMDB and RocksDB hold bytes. The trust flag, the classification, the ceilings and capability narrowing stop at their API, so a program could not carry them to disk. A native store keeps them in the bytes and stays inside ADR 0012, which writes TLS, git and crypto in Oska. A log is small enough to model and to test with crash injection, and old states are prefixes of it, which gives tamper evidence and time travel.

## Considered options

- **A runtime for `std.sql` over SQLite first.** Rejected: labels stop at the library boundary, and it adds a foreign engine to audit.
- **Both at once.** Rejected for now: two surfaces to secure, and the native store is the one that matters for agents.
- **The label from the file only, or from the capability only.** Rejected: the first can return data below the capability's level, and the second ignores per-record labels.
- **Typed queries and a SQL subset together.** Rejected for now: Oska has no user macros (ADR 0003), so a SQL subset needs a parser in the compiler.
- **The engine and the model in parallel.** Rejected: the engine could ship a primitive that the model then shows to be wrong.

## Consequences

- No store ships before the file primitives and the TLA+ model agree. Without sync and atomic rename, a durability claim would be false.
- Constant time holds for native code only, and classification gives no timing guarantee (ADR 0029).
- The compliance reports treat encryption at rest as a self-assessment item (ADR 0017), not as a validated claim.
- The interpreter and every native backend must give the same store behaviour (ADR 0014).
- Search, a hash-chained audit head, the MCP tools and agent memory are later specs. See `docs/research/2026-11-ai-first-data-stores.md`.
