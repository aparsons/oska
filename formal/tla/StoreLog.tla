------------------------------ MODULE StoreLog ------------------------------
(* The crash model of `std.store` (ADR 0040, spec #842): an append-only,  *)
(* hash-chained log, a snapshot that lets the log's front be dropped, and *)
(* a crash that loses what no sync made durable. store-log.md beside it   *)
(* is the design this model checks and the file operations it fixes.      *)
(*                                                                         *)
(* Records are numbered from 1 and never change. The writer:               *)
(*  - Put: appends a record with a label to the log file (in the cache).   *)
(*  - Sync: makes every record appended so far durable.                    *)
(*  - Ack: tells the caller a write is done. It is allowed only for a      *)
(*    record that Sync made durable (AckAfterSync).                        *)
(*  - Snapshot, in order: write a temporary file with the first logSync    *)
(*    records (SnapTmp), sync its data (SyncSnap), rename it to the        *)
(*    snapshot's name (Rename), sync the directory so the rename is        *)
(*    durable (SyncDir), and only then drop those records from the log     *)
(*    file (Truncate). SyncSnapshotFirst makes Rename wait for SyncSnap.   *)
(* A crash at any step keeps, of the log file, the durable records and any *)
(* of the records after them, and the last of those may be torn. A rename  *)
(* that no directory sync has made durable may or may not have happened.   *)
(* A snapshot whose data no sync made durable may be torn. Then the store  *)
(* opens: it takes the snapshot if its checksum holds (VerifyChecksum),    *)
(* then reads the log as far as each record's checksum and its place in    *)
(* the chain hold. A torn record has no valid checksum.                    *)
(*                                                                         *)
(* The model assumes dropping the log's front is atomic and durable (the   *)
(* log is a sequence of files and the old one is removed with a directory  *)
(* sync); store-log.md says so. Tampering with a file is not modelled: the *)
(* attack tests of the engine cover it.                                    *)
EXTENDS Naturals, Sequences

CONSTANTS
  MaxWrites,          \* the most records one run writes
  MaxLabel,           \* labels are 0 .. MaxLabel
  AckAfterSync,       \* TRUE: a write is acknowledged only after Sync
  SyncSnapshotFirst,  \* TRUE: the snapshot's data is synced before Rename
  VerifyChecksum      \* TRUE: open refuses a record or snapshot that is torn

VARIABLES
  written,     \* every record written, in order; the first logSync are durable
  logSync,     \* records durable in the log file
  acked,       \* writes acknowledged to the caller
  base,        \* records dropped from the front of the log file
  phase,       \* snapshot: "none", "tmp", "renamed", "durable", "truncated"
  snapN,       \* records the snapshot covers
  dataSynced,  \* the snapshot's data is durable
  crashed,     \* the machine crashed and the store opened again
  recovered    \* the records open found

vars == <<written, logSync, acked, base, phase, snapN, dataSynced,
          crashed, recovered>>

Min2(a, b) == IF a < b THEN a ELSE b

Rec == [lab : 0 .. MaxLabel, ok : BOOLEAN]
Torn == [lab |-> 0, ok |-> FALSE]

TypeOK ==
  /\ written \in Seq(Rec)
  /\ Len(written) <= MaxWrites
  /\ \A i \in 1 .. Len(written) : written[i].ok
  /\ logSync \in 0 .. Len(written)
  /\ acked \in 0 .. Len(written)
  /\ base \in 0 .. logSync
  /\ phase \in {"none", "tmp", "renamed", "durable", "truncated"}
  /\ snapN \in 0 .. logSync
  /\ dataSynced \in BOOLEAN
  /\ crashed \in BOOLEAN
  /\ recovered \in Seq(Rec)

Init ==
  /\ written = <<>>
  /\ logSync = 0
  /\ acked = 0
  /\ base = 0
  /\ phase = "none"
  /\ snapN = 0
  /\ dataSynced = FALSE
  /\ crashed = FALSE
  /\ recovered = <<>>

Running == ~crashed

Put(l) ==
  /\ Running
  /\ Len(written) < MaxWrites
  /\ written' = Append(written, [lab |-> l, ok |-> TRUE])
  /\ UNCHANGED <<logSync, acked, base, phase, snapN, dataSynced, crashed,
                 recovered>>

Sync ==
  /\ Running
  /\ logSync < Len(written)
  /\ logSync' = Len(written)
  /\ UNCHANGED <<written, acked, base, phase, snapN, dataSynced, crashed,
                 recovered>>

Ack ==
  /\ Running
  /\ acked < IF AckAfterSync THEN logSync ELSE Len(written)
  /\ acked' = acked + 1
  /\ UNCHANGED <<written, logSync, base, phase, snapN, dataSynced, crashed,
                 recovered>>

SnapTmp ==
  /\ Running
  /\ phase = "none"
  /\ logSync > 0
  /\ phase' = "tmp"
  /\ snapN' = logSync
  /\ UNCHANGED <<written, logSync, acked, base, dataSynced, crashed,
                 recovered>>

SyncSnap ==
  /\ Running
  /\ phase \in {"tmp", "renamed", "durable", "truncated"}
  /\ ~dataSynced
  /\ dataSynced' = TRUE
  /\ UNCHANGED <<written, logSync, acked, base, phase, snapN, crashed,
                 recovered>>

Rename ==
  /\ Running
  /\ phase = "tmp"
  /\ dataSynced \/ ~SyncSnapshotFirst
  /\ phase' = "renamed"
  /\ UNCHANGED <<written, logSync, acked, base, snapN, dataSynced, crashed,
                 recovered>>

SyncDir ==
  /\ Running
  /\ phase = "renamed"
  /\ phase' = "durable"
  /\ UNCHANGED <<written, logSync, acked, base, snapN, dataSynced, crashed,
                 recovered>>

Truncate ==
  /\ Running
  /\ phase = "durable"
  /\ phase' = "truncated"
  /\ base' = snapN
  /\ UNCHANGED <<written, logSync, acked, snapN, dataSynced, crashed,
                 recovered>>

\* What open finds. snap is the snapshot's records (empty when it is absent
\* or refused). It reads the log file's records base+1 .. k, the next one
\* only when it continues the chain and, with VerifyChecksum, is not torn.
RECURSIVE Scan(_, _, _, _)
Scan(rec, i, k, torn) ==
  IF i > k THEN rec
  ELSE LET d == IF torn /\ i = k /\ k > logSync THEN Torn ELSE written[i]
       IN IF i <= Len(rec) THEN Scan(rec, i + 1, k, torn)
          ELSE IF i = Len(rec) + 1 /\ (d.ok \/ ~VerifyChecksum)
               THEN Scan(Append(rec, d), i + 1, k, torn)
               ELSE rec

\* Which snapshot a crash leaves under the snapshot's name.
NewSnapshotVisible ==
  IF phase \in {"durable", "truncated"} THEN {TRUE}
  ELSE IF phase = "renamed" THEN BOOLEAN
  ELSE {FALSE}

SnapshotIntact == IF dataSynced THEN {TRUE} ELSE BOOLEAN

Crash ==
  /\ Running
  /\ \E k \in logSync .. Len(written), torn \in BOOLEAN,
        useNew \in NewSnapshotVisible, intact \in SnapshotIntact :
       LET snap == IF ~useNew THEN <<>>
                   ELSE IF intact THEN SubSeq(written, 1, snapN)
                   ELSE IF VerifyChecksum THEN <<>>
                   ELSE [i \in 1 .. snapN |-> Torn]
       IN recovered' = Scan(snap, base + 1, k, torn)
  /\ crashed' = TRUE
  /\ UNCHANGED <<written, logSync, acked, base, phase, snapN, dataSynced>>

Done == crashed /\ UNCHANGED vars

Next ==
  \/ \E l \in 0 .. MaxLabel : Put(l)
  \/ Sync \/ Ack \/ SnapTmp \/ SyncSnap \/ Rename \/ SyncDir \/ Truncate
  \/ Crash
  \/ Done

Spec == Init /\ [][Next]_vars

\* A write that was acknowledged is in what open finds.
AckedSurvive ==
  crashed =>
    /\ Len(recovered) >= acked
    /\ \A i \in 1 .. acked : recovered[i] = written[i]

\* What open finds is the first records of what was written, unchanged.
RecoveryIsPrefix ==
  crashed =>
    /\ Len(recovered) <= Len(written)
    /\ \A i \in 1 .. Len(recovered) : recovered[i] = written[i]

\* Every record that open takes has a valid checksum and chain link.
ChainValid ==
  crashed => \A i \in 1 .. Len(recovered) : recovered[i].ok

\* No record comes back with a lower label than it was written with.
NoLabelDowngrade ==
  crashed =>
    \A i \in 1 .. Min2(Len(recovered), Len(written)) :
      recovered[i].lab >= written[i].lab

\* The log's front is dropped only when a durable, intact snapshot covers it.
TruncateOnlyAfterDurableSnapshot ==
  base > 0 => (phase = "truncated" /\ dataSynced /\ base <= snapN)

=============================================================================
