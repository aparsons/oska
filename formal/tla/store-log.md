# StoreLog: the crash model of `std.store`

Design note for `std.store` (ADR 0040, spec #842). `StoreLog.tla` is the model, and TLC checks it in the `formal` step of `scripts/check.sh`. ADR 0040 says no store code ships before this model and the file primitives agree. This note lists what the model checks and the order of file operations that the prelude must offer.

## The models

| Model | What it covers | Bound | TLC in the formal step |
| --- | --- | --- | --- |
| `StoreLog` | an append-only log, a snapshot that lets the log's front be dropped, a crash, open | up to 3 records with labels 0 and 1, one snapshot, one crash | about 1 s (8,898 states) |
| `StoreLogAckEarly` | `StoreLog` that acknowledges a write before its sync (broken on purpose) | as `StoreLog` | fails: `AckedSurvive` violated in a 4-state trace |
| `StoreLogRenameFirst` | `StoreLog` that renames the snapshot before syncing its data (broken on purpose) | as `StoreLog` | fails: `TruncateOnlyAfterDurableSnapshot` violated in a 7-state trace; `AckedSurvive` fails too, one crash later |
| `StoreLogNoChecksum` | `StoreLog` whose open accepts a torn record (broken on purpose) | as `StoreLog` | fails: `ChainValid` violated in a 3-state trace |

The broken models follow the rule in `docs/agents/formal.md`: a `.cfg` that starts with `\* expect-violation: NAME` passes the step only when TLC finds `NAME` violated.

## Properties

| Property | Kind | Says |
| --- | --- | --- |
| `AckedSurvive` | invariant | a write that was acknowledged is in what open finds after a crash |
| `RecoveryIsPrefix` | invariant | what open finds is the first records of what was written, unchanged |
| `ChainValid` | invariant | every record that open takes has a valid checksum and chain link |
| `NoLabelDowngrade` | invariant | no record comes back with a lower label than it was written with |
| `TruncateOnlyAfterDurableSnapshot` | invariant | the log's front is dropped only after a durable, intact snapshot covers it |

## What the crash may do

- Of the log file, the records that a sync made durable stay. Of the records after them, any first few stay, and the last of those may be torn (it has no valid checksum). Records reach the disk in order, so a record never stays while an earlier one is lost. A real file system does not promise that. The chain check makes it harmless: open stops at the first record that does not continue the chain.
- A snapshot whose data no sync made durable may be torn.
- A rename that no directory sync made durable may or may not have happened.
- Not modelled: a file edited on purpose. That is the attack tests' job (spec #842, ticket "Tamper and crash attack tests").
- Assumed, and not modelled: dropping the log's front (`Truncate`) is atomic and durable. The log is a sequence of files, and the engine removes an old one and syncs the directory. The ticket that builds the engine either keeps this true or extends the model first. `Store.snapshot` (#851) keeps it true without a sequence of files: it drops the front by replacing `log` with its last record through a temporary file, `rename` and a directory sync, which the file API makes atomic and, after the sync, durable. The last record stays so that a deleted snapshot shows as a log that starts after record 1. What the file API does not promise is that `remove_file` or `rename` is durable before the directory sync; the engine syncs the directory after each, and `CrashHost` tests it at every boundary.

## The operation order the file API must offer

The prelude needs these confined capability methods (ADR 0008), each on the interpreter and every native backend (ADR 0014), and the engine must call them in this order.

1. **Write a record.** `append` the sealed record to the log file (`FileWrite`).
2. **Acknowledge.** `sync` the log file first. Only after it returns may the store say the write is done. `AckAfterSync` is this rule; without it a crash loses an acknowledged write.
3. **Snapshot**, all five steps in this order:
   1. Write the snapshot to a temporary file in the same directory. It covers only records that step 2 made durable.
   2. `sync` the temporary file's data.
   3. `rename` it over the snapshot's name (atomic). `SyncSnapshotFirst` is the rule that this comes after step 3.2: otherwise a crash leaves the new name with torn data.
   4. `sync` the directory, so the rename is durable.
   5. Only then remove the log's front.
4. **Open**: read the snapshot if its checksum holds, then read the log from the record after it, and stop at the first record whose checksum or chain link fails. `VerifyChecksum` is this rule.

So the primitives are: `append`, file `sync`, `rename` inside one capability, directory `sync`, `remove_file`, and the exclusive lock that gives one writer. `append`, `rename` and `remove_file` are ticket #679; file `sync` and directory `sync` are the next ticket of the spec; the lock is `DirWrite.with_lock(f)` (#846): scoped, because Oska has no destructors, it holds `.oska-lock` in the directory for the length of `f`, and a second lock fails with `L0001`.

### Which method is which step

| Step of the order | Method | Ticket |
| --- | --- | --- |
| 1 Write a record | `FileWrite.append(text)` | #679 |
| 2 Acknowledge: `sync` the log | `FileWrite.sync()` | #845 |
| 3.1 Snapshot to a temporary file | `FileWrite.write_bytes(bytes)` or `DirWrite.write_bytes(path, bytes)` | existing |
| 3.2 `sync` the temporary file | `FileWrite.sync()` | #845 |
| 3.3 `rename` over the snapshot's name | `DirWrite.rename(from, to)` | #679 |
| 3.4 `sync` the directory | `DirWrite.sync()` | #845 |
| 3.5 Remove the log's front | `DirWrite.remove_file(path)` | #679 |
| One writer | `DirWrite.with_lock(f)` | #846 |

`FileWrite.sync` on macOS is `F_FULLFSYNC`, so `Ok` means the drive's cache is empty too; the model's "durable" is that. On Windows a directory flush is not documented to make a rename durable (`docs/runtime.md`), so step 3.4 there rests on what NTFS's journal gives.

The model has one writer. The lock capability is what makes that true, and a second writer is out of scope for the first version.

## The crash filesystem (`oska_run::CrashHost`, #847)

`crates/oska-run/src/crash_fs.rs` runs a program on an in-memory filesystem, records each step, and replays the record into every crash the rules below allow. `crates/oska-run/tests/crash_fs.rs` proves it against the models: a write acknowledged after its sync survives every crash; one acknowledged before it (`StoreLogAckEarly`) is lost at some crash; a rename before the directory sync can revert; a rename before the data sync (`StoreLogRenameFirst`) can leave a torn snapshot; a torn last append leaves a prefix.

How it mirrors the model:

- Data: `write` and `append` are cached; a file `sync` makes that file's data durable. A crash keeps any first few unsynced writes and may tear the next one to a prefix (`Tears`: none, half, or every prefix). This is "any first few of the unsynced ones, the last possibly torn".
- Names: a `rename`, `remove_file` or `remove_dir` is visible at once and durable after the sync of each directory it changes. A crash keeps any first few unsynced changes: an unsynced rename may or may not have happened.
- Data and names are chosen independently, so a rename can survive with the file's data not: the torn snapshot of `StoreLogRenameFirst`.
- Acknowledged: stdout is the acknowledgement, and each print is a crash point.

Stricter than the model: a crash is a point between two steps, never inside one `append` (a torn write is the tear choice).

Looser than the model, and than a real disk:

- Unsynced writes reach the disk in program order across all files; a real file system may keep a later write and lose an earlier one. The chain check makes that harmless in the model, and this filesystem does not try it.
- A new file's name and `create_dir` are durable at once (the model has no creation to lose); only its data is not. A creation after an unsynced change of the same name waits for the directory sync.
- Unsynced name changes are kept as a prefix in order, not as any subset. A change across two directories needs both syncs, and blocks later changes until then.
- A torn `write` over an existing file is a prefix of the new bytes; it is never emptied or mixed with the old ones.
- No bit rot or edited file: that is the attack tests' job.

An Oska test program under `oskac test` cannot use it yet: `t.fs` is a `FakeHost` inside `TestHost`, and a crash needs a new prelude seam (a capability to crash and reopen) in the checker, the interpreter and every native backend. It is left to a later ticket.
