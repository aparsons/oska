----------------------------- MODULE SharedCounts -----------------------------
(* Reference counts on values that tasks running in parallel share (ADRs   *)
(* 0001 and 0019, issue #432). parallel-tasks.md beside it is the design   *)
(* this model checks.                                                      *)
(*                                                                         *)
(* Each task holds references to values: held[t][v] is how many of t's     *)
(* bindings name v. A task may at any step:                                *)
(*  - Share: give a new reference to v to any task, itself included. This  *)
(*    is a capture at spawn, a result at join, or `let y = x`. Only a      *)
(*    task that holds v can share it, so a count goes up only while it is  *)
(*    at least 1.                                                          *)
(*  - Drop: give up a reference; the value is freed when its count reaches *)
(*    0.                                                                   *)
(*  - Write through a `var` (ADR 0001): load the count; if it was 1, change *)
(*    the value in place, else copy it, change the copy, and drop the      *)
(*    reference to the original.                                           *)
(* This lets any task share with any other at any time, which covers every *)
(* way ScopedTasks can pass values between tasks.                          *)
(*                                                                         *)
(* With Atomic, each count update is one atomic read-modify-write. Without *)
(* it (SharedCountsRacy), each is a load and then a separate store, and    *)
(* TLC must find a value freed while a task still holds it. The check      *)
(* before a write is a separate load in both, as an implementation does.   *)
EXTENDS Naturals, FiniteSets

CONSTANTS
  Tasks,      \* the tasks
  Values,     \* value ids; the first is live at the start, the rest are free
  MaxRefs,    \* the most references one task holds to one value
  MaxWrites,  \* the most writes in one run
  Atomic      \* TRUE: counts change by atomic read-modify-write

VARIABLES
  status,   \* status[v]: "unused", "live", "freed"
  count,    \* count[v]: v's reference count
  held,     \* held[t][v]: the references t holds to v
  content,  \* content[v]: v's contents, a version that each write bumps
  seen,     \* seen[t][v]: the contents t last saw in v (by sharing or writing)
  pc,       \* pc[t]: the step t is in the middle of ("idle" between steps)
  reg,      \* reg[t]: the count t loaded and has not stored yet
  arg,      \* arg[t]: the value (and task) t's current step is about
  writes    \* writes started so far

vars == <<status, count, held, content, seen, pc, reg, arg, writes>>

First == CHOOSE v \in Values : \A w \in Values : v <= w

TypeOK ==
  /\ status \in [Values -> {"unused", "live", "freed"}]
  /\ count \in [Values -> Nat]
  /\ held \in [Tasks -> [Values -> 0 .. MaxRefs]]
  /\ content \in [Values -> 0 .. MaxWrites]
  /\ seen \in [Tasks -> [Values -> 0 .. MaxWrites]]
  /\ pc \in [Tasks -> {"idle", "share", "check", "release"}]
  /\ reg \in [Tasks -> 0 .. Cardinality(Tasks) * MaxRefs]
  /\ arg \in [Tasks -> Values \X Tasks]
  /\ writes \in 0 .. MaxWrites

Init ==
  /\ status = [v \in Values |-> IF v = First THEN "live" ELSE "unused"]
  /\ count = [v \in Values |-> IF v = First THEN Cardinality(Tasks) ELSE 0]
  /\ held = [t \in Tasks |-> [v \in Values |-> IF v = First THEN 1 ELSE 0]]
  /\ content = [v \in Values |-> 0]
  /\ seen = [t \in Tasks |-> [v \in Values |-> 0]]
  /\ pc = [t \in Tasks |-> "idle"]
  /\ reg = [t \in Tasks |-> 0]
  /\ arg = [t \in Tasks |-> <<First, t>>]
  /\ writes = 0

Idle(t) == pc[t] = "idle"
Holds(t, v) == held[t][v] > 0

\* A count update. Atomic: it happens in the step that starts it. Not
\* atomic: that step loads the count into reg[t], and a later step
\* (ShareStore, ReleaseStore) stores reg[t] plus or minus 1.
Free(v, n) == status' = [status EXCEPT ![v] = IF n = 0 THEN "freed" ELSE @]

\* t's step is over; its registers are cleared, so they do not split states.
ToIdle(t) ==
  /\ pc' = [pc EXCEPT ![t] = "idle"]
  /\ reg' = [reg EXCEPT ![t] = 0]
  /\ arg' = [arg EXCEPT ![t] = <<First, t>>]

\* What t saw in v matters only while t holds v.
SeenAfterDrop(t, v) == IF held[t][v] = 1 THEN 0 ELSE seen[t][v]

\* --- Share ------------------------------------------------------------------

\* The new reference is given once the count covers it.
GiveRef(u, v, from) ==
  /\ held' = [held EXCEPT ![u][v] = @ + 1]
  /\ seen' = [seen EXCEPT ![u][v] = seen[from][v]]

Share(t, u, v) ==
  /\ Idle(t)
  /\ Holds(t, v)
  /\ held[u][v] < MaxRefs
  /\ IF Atomic
       THEN /\ count' = [count EXCEPT ![v] = @ + 1]
            /\ GiveRef(u, v, t)
            /\ UNCHANGED <<pc, reg, arg>>
       ELSE /\ reg' = [reg EXCEPT ![t] = count[v]]
            /\ pc' = [pc EXCEPT ![t] = "share"]
            /\ arg' = [arg EXCEPT ![t] = <<v, u>>]
            /\ UNCHANGED <<count, held, seen>>
  /\ UNCHANGED <<status, content, writes>>

ShareStore(t) ==
  /\ pc[t] = "share"
  /\ LET v == arg[t][1]  u == arg[t][2] IN
       /\ count' = [count EXCEPT ![v] = reg[t] + 1]
       /\ GiveRef(u, v, t)
  /\ ToIdle(t)
  /\ UNCHANGED <<status, content, writes>>

\* --- Drop and release -------------------------------------------------------

\* Give up one reference to v and take one off its count, freeing v at 0.
\* Not atomic: the reference goes in the first step, the store comes later.
Release(t, v) ==
  IF Atomic
    THEN /\ count' = [count EXCEPT ![v] = @ - 1]
         /\ Free(v, count[v] - 1)
         /\ UNCHANGED <<pc, reg, arg>>
    ELSE /\ reg' = [reg EXCEPT ![t] = count[v]]
         /\ pc' = [pc EXCEPT ![t] = "release"]
         /\ arg' = [arg EXCEPT ![t] = <<v, t>>]
         /\ UNCHANGED <<count, status>>

Drop(t, v) ==
  /\ Idle(t)
  /\ Holds(t, v)
  /\ held' = [held EXCEPT ![t][v] = @ - 1]
  /\ seen' = [seen EXCEPT ![t][v] = SeenAfterDrop(t, v)]
  /\ Release(t, v)
  /\ UNCHANGED <<content, writes>>

ReleaseStore(t) ==
  /\ pc[t] = "release"
  /\ LET v == arg[t][1] IN
       /\ count' = [count EXCEPT ![v] = reg[t] - 1]
       /\ Free(v, reg[t] - 1)
  /\ ToIdle(t)
  /\ UNCHANGED <<held, content, seen, writes>>

\* --- Write through a `var` -------------------------------------------------

\* The check: one load of the count. It counts the write against the bound.
Check(t, v) ==
  /\ Idle(t)
  /\ Holds(t, v)
  /\ writes < MaxWrites
  /\ reg' = [reg EXCEPT ![t] = count[v]]
  /\ arg' = [arg EXCEPT ![t] = <<v, t>>]
  /\ pc' = [pc EXCEPT ![t] = "check"]
  /\ writes' = writes + 1
  /\ UNCHANGED <<status, count, held, content, seen>>

\* Count 1: change v in place.
WriteInPlace(t) ==
  /\ pc[t] = "check"
  /\ reg[t] = 1
  /\ LET v == arg[t][1] IN
       /\ content' = [content EXCEPT ![v] = @ + 1]
       /\ seen' = [seen EXCEPT ![t][v] = content[v] + 1]
  /\ ToIdle(t)
  /\ UNCHANGED <<status, count, held, writes>>

\* Count above 1: copy v to a new value w with count 1, change the copy,
\* then release the reference to v. With no unused id left (the bound),
\* the write is not made.
WriteCopy(t) ==
  /\ pc[t] = "check"
  /\ reg[t] # 1
  /\ LET v == arg[t][1] IN
       IF \E w \in Values : status[w] = "unused"
         THEN LET w == CHOOSE x \in Values : status[x] = "unused" IN
              /\ content' = [content EXCEPT ![w] = content[v] + 1]
              /\ seen' = [seen EXCEPT ![t][w] = content[v] + 1,
                                     ![t][v] = SeenAfterDrop(t, v)]
              /\ held' = [held EXCEPT ![t][v] = @ - 1, ![t][w] = 1]
              /\ IF Atomic
                   THEN /\ count' = [count EXCEPT ![w] = 1, ![v] = @ - 1]
                        /\ status' = [status EXCEPT ![w] = "live",
                                        ![v] = IF count[v] = 1 THEN "freed" ELSE @]
                        /\ ToIdle(t)
                   ELSE /\ count' = [count EXCEPT ![w] = 1]
                        /\ status' = [status EXCEPT ![w] = "live"]
                        /\ reg' = [reg EXCEPT ![t] = count[v]]
                        /\ pc' = [pc EXCEPT ![t] = "release"]
                        /\ UNCHANGED arg
         ELSE /\ ToIdle(t)
              /\ UNCHANGED <<status, count, held, content, seen>>
  /\ UNCHANGED writes

\* Every reference is gone: nothing more happens.
Ended ==
  /\ \A t \in Tasks : Idle(t) /\ \A v \in Values : held[t][v] = 0
  /\ UNCHANGED vars

Next ==
  \/ \E t \in Tasks :
       \/ \E u \in Tasks, v \in Values : Share(t, u, v)
       \/ \E v \in Values : Drop(t, v) \/ Check(t, v)
       \/ ShareStore(t) \/ ReleaseStore(t) \/ WriteInPlace(t) \/ WriteCopy(t)
  \/ Ended

Spec == Init /\ [][Next]_vars

\* --- properties --------------------------------------------------------------

\* No value is freed while in use: every value a task holds is live.
NoUseAfterFree ==
  \A t \in Tasks, v \in Values : Holds(t, v) => status[v] = "live"

\* No in-place change is visible to another task: every task sees in each
\* value it holds the contents it last saw there.
NoVisibleInPlaceChange ==
  \A t \in Tasks, v \in Values : Holds(t, v) => seen[t][v] = content[v]

\* A count is exactly the references held: so a value is freed when, and
\* only when, its last reference goes (no early free, no leak).
RECURSIVE Sum(_, _)
Sum(S, v) ==
  IF S = {} THEN 0
  ELSE LET t == CHOOSE x \in S : TRUE IN held[t][v] + Sum(S \ {t}, v)
CountIsReferences ==
  \A v \in Values :
    /\ status[v] = "live" => count[v] = Sum(Tasks, v) /\ count[v] > 0
    /\ status[v] # "live" => count[v] = 0 /\ Sum(Tasks, v) = 0
=============================================================================
