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
- Assumed, and not modelled: dropping the log's front (`Truncate`) is atomic and durable. The log is a sequence of files, and the engine removes an old one and syncs the directory. The ticket that builds the engine either keeps this true or extends the model first.

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

The model has one writer. The lock capability is what makes that true, and a second writer is out of scope for the first version.
