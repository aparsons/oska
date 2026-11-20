----------------------------- MODULE ScopedTasks -----------------------------
(* Parallel scoped tasks (ADR 0019, issue #432): a scheduler that runs     *)
(* tasks on a fixed number of workers in parallel, scopes that wait for    *)
(* their tasks, and join. parallel-tasks.md beside it is the design that   *)
(* this model checks.                                                      *)
(*                                                                         *)
(* Every program up to the bound is in the state space: each running task  *)
(* may at any step open a scope, spawn into any scope it holds, join any   *)
(* task whose handle it holds, end its innermost scope, or return. A task  *)
(* holds a handle only the way an Oska program can get one: it spawned the *)
(* task, it captured the handle when it was spawned, or a task it joined   *)
(* returned it. A spawned task captures every handle its spawner holds and *)
(* a joined task returns every handle it holds, which allows the most     *)
(* joins, so this is the worst case for deadlock.                          *)
(*                                                                         *)
(* A task that waits (join, or the end of a scope whose tasks still run)   *)
(* parks: it gives up its worker, and a worker never runs a task inside    *)
(* another task. Issue #482 is the sequential v0 runtime breaking that     *)
(* rule.                                                                   *)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
  MaxTasks,   \* task ids 0 .. MaxTasks-1; task 0 is `main`
  MaxScopes,  \* scope ids 1 .. MaxScopes
  Workers,    \* the scheduler's workers (OS threads)
  ConfineScopes
    \* TRUE: a task spawned on scope s captures only the handles of s and
    \* the scopes around s. FALSE: it captures every scope handle its
    \* spawner holds, which today's checker allows (see parallel-tasks.md).

Main == 0
TaskIds == 0 .. MaxTasks - 1
ScopeIds == 1 .. MaxScopes
None == 0 - 1  \* no task, no scope

VARIABLES
  st,       \* st[t]: "unused", "ready", "running", "joining", "closing", "done"
  home,     \* home[t]: the scope t was spawned into (None for main)
  knows,    \* knows[t]: the task handles t holds
  scopes,   \* scopes[t]: the scope handles t holds
  stack,    \* stack[t]: the scopes t opened and has not ended, innermost last
  waits,    \* waits[t]: the task t joins, or the scope t is ending
  sst,      \* sst[s]: "unused", "open", "closed"
  owner,    \* owner[s]: the task whose block s is
  around    \* around[s]: s and the scopes whose blocks enclose s's block

vars == <<st, home, knows, scopes, stack, waits, sst, owner, around>>

TypeOK ==
  /\ st \in [TaskIds -> {"unused", "ready", "running", "joining", "closing", "done"}]
  /\ home \in [TaskIds -> ScopeIds \cup {None}]
  /\ knows \in [TaskIds -> SUBSET TaskIds]
  /\ scopes \in [TaskIds -> SUBSET ScopeIds]
  /\ \A t \in TaskIds : stack[t] \in Seq(ScopeIds)
  /\ waits \in [TaskIds -> TaskIds \cup ScopeIds \cup {None}]
  /\ sst \in [ScopeIds -> {"unused", "open", "closed"}]
  /\ owner \in [ScopeIds -> TaskIds \cup {None}]
  /\ around \in [ScopeIds -> SUBSET ScopeIds]

Init ==
  /\ st = [t \in TaskIds |-> IF t = Main THEN "ready" ELSE "unused"]
  /\ home = [t \in TaskIds |-> None]
  /\ knows = [t \in TaskIds |-> {}]
  /\ scopes = [t \in TaskIds |-> {}]
  /\ stack = [t \in TaskIds |-> <<>>]
  /\ waits = [t \in TaskIds |-> None]
  /\ sst = [s \in ScopeIds |-> "unused"]
  /\ owner = [s \in ScopeIds |-> None]
  /\ around = [s \in ScopeIds |-> {}]

Members(s) == {t \in TaskIds : home[t] = s}
AllDone(s) == \A m \in Members(s) : st[m] = "done"
Running(t) == st[t] = "running"
\* A task holds a worker exactly while it runs; the workers are alike, so
\* the model counts them instead of naming them.
Busy == Cardinality({t \in TaskIds : Running(t)})

\* --- the scheduler -----------------------------------------------------------

\* A free worker takes a ready task.
Schedule(t) ==
  /\ Busy < Workers
  /\ st[t] = "ready"
  /\ st' = [st EXCEPT ![t] = "running"]
  /\ UNCHANGED <<home, knows, scopes, stack, waits, sst, owner, around>>

\* A joined task has finished: the joiner is ready again and holds the
\* handles the result carries.
WakeJoin(t) ==
  /\ st[t] = "joining"
  /\ st[waits[t]] = "done"
  /\ st' = [st EXCEPT ![t] = "ready"]
  /\ knows' = [knows EXCEPT ![t] = @ \cup knows[waits[t]]]
  /\ waits' = [waits EXCEPT ![t] = None]
  /\ UNCHANGED <<home, scopes, stack, sst, owner, around>>

\* Every task of the scope being ended has finished: the scope closes and
\* its owner is ready again.
WakeClose(t) ==
  /\ st[t] = "closing"
  /\ AllDone(waits[t])
  /\ sst' = [sst EXCEPT ![waits[t]] = "closed"]
  /\ stack' = [stack EXCEPT ![t] = SubSeq(@, 1, Len(@) - 1)]
  /\ scopes' = [scopes EXCEPT ![t] = @ \ {waits[t]}]
  /\ st' = [st EXCEPT ![t] = "ready"]
  /\ waits' = [waits EXCEPT ![t] = None]
  /\ UNCHANGED <<home, knows, owner, around>>

\* --- what a running task does ---------------------------------------------

\* `scope |s| { ... }` starts.
OpenScope(t) ==
  /\ Running(t)
  /\ \E s \in ScopeIds :
       /\ sst[s] = "unused"
       /\ \A s2 \in ScopeIds : s2 < s => sst[s2] # "unused"
       /\ sst' = [sst EXCEPT ![s] = "open"]
       /\ owner' = [owner EXCEPT ![s] = t]
       /\ around' = [around EXCEPT ![s] =
            {s} \cup {stack[t][i] : i \in 1 .. Len(stack[t])}
                \cup (IF home[t] = None THEN {} ELSE around[home[t]])]
       /\ stack' = [stack EXCEPT ![t] = Append(@, s)]
       /\ scopes' = [scopes EXCEPT ![t] = @ \cup {s}]
  /\ UNCHANGED <<st, home, knows, waits>>

\* `s.spawn(|| ...)`: the new task captures the task handles its spawner
\* holds and its scope handles (with ConfineScopes, only those of s and the
\* scopes around s), and the spawner gets the new task's handle.
Spawn(t, s) ==
  /\ Running(t)
  /\ s \in scopes[t]
  /\ \E u \in TaskIds :
       /\ st[u] = "unused"
       /\ \A u2 \in TaskIds : u2 < u => st[u2] # "unused"
       /\ st' = [st EXCEPT ![u] = "ready"]
       /\ home' = [home EXCEPT ![u] = s]
       /\ knows' = [knows EXCEPT ![u] = knows[t], ![t] = @ \cup {u}]
       /\ scopes' = [scopes EXCEPT ![u] =
            IF ConfineScopes THEN scopes[t] \cap around[s] ELSE scopes[t]]
  /\ UNCHANGED <<stack, waits, sst, owner, around>>

\* `h.join()`: the result of a finished task, else park until it finishes.
Join(t, k) ==
  /\ Running(t)
  /\ k \in knows[t]
  /\ IF st[k] = "done"
       THEN /\ knows' = [knows EXCEPT ![t] = @ \cup knows[k]]
            /\ UNCHANGED <<st, waits>>
       ELSE /\ st' = [st EXCEPT ![t] = "joining"]
            /\ waits' = [waits EXCEPT ![t] = k]
            /\ UNCHANGED knows
  /\ UNCHANGED <<home, scopes, stack, sst, owner, around>>

\* The innermost scope's block ends: it closes if its tasks have finished,
\* else its owner parks until they have.
EndScope(t) ==
  /\ Running(t)
  /\ stack[t] # <<>>
  /\ LET s == stack[t][Len(stack[t])] IN
       IF AllDone(s)
         THEN /\ sst' = [sst EXCEPT ![s] = "closed"]
              /\ stack' = [stack EXCEPT ![t] = SubSeq(@, 1, Len(@) - 1)]
              /\ scopes' = [scopes EXCEPT ![t] = @ \ {s}]
              /\ UNCHANGED <<st, waits>>
         ELSE /\ st' = [st EXCEPT ![t] = "closing"]
              /\ waits' = [waits EXCEPT ![t] = s]
              /\ UNCHANGED <<sst, stack, scopes>>
  /\ UNCHANGED <<home, knows, owner, around>>

\* The task's body returns. Its scope blocks have ended, as blocks nest.
Return(t) ==
  /\ Running(t)
  /\ stack[t] = <<>>
  /\ st' = [st EXCEPT ![t] = "done"]
  /\ UNCHANGED <<home, knows, scopes, stack, waits, sst, owner, around>>

\* `main` has returned: the program has ended, and nothing more happens.
Ended == st[Main] = "done" /\ UNCHANGED vars

TaskStep(t) ==
  \/ OpenScope(t)
  \/ \E s \in ScopeIds : Spawn(t, s)
  \/ \E k \in TaskIds : Join(t, k)
  \/ EndScope(t)
  \/ Return(t)

Next ==
  \/ \E t \in TaskIds : Schedule(t)
  \/ \E t \in TaskIds : WakeJoin(t) \/ WakeClose(t) \/ TaskStep(t)
  \/ Ended

\* Each task keeps taking steps while it can, the scheduler wakes a waiting
\* task once it may go on, and a ready task gets a worker even when other
\* tasks keep taking the free ones (strong fairness).
Fairness ==
  \A t \in TaskIds :
    /\ WF_vars(TaskStep(t))
    /\ WF_vars(WakeJoin(t))
    /\ WF_vars(WakeClose(t))
    /\ SF_vars(Schedule(t))

Spec == Init /\ [][Next]_vars /\ Fairness

\* --- properties --------------------------------------------------------------

\* Every scope ends after its tasks: a closed scope has no task still to
\* finish.
ScopeEndsAfterItsTasks ==
  \A s \in ScopeIds : sst[s] = "closed" => AllDone(s)

\* A task returns only after every scope it opened has closed, so `main`
\* returns last.
TaskEndsAfterItsScopes ==
  \A t \in TaskIds : st[t] = "done" =>
    \A s \in ScopeIds : owner[s] = t => sst[s] = "closed"
MainEndsLast ==
  st[Main] = "done" => \A t \in TaskIds : st[t] \in {"unused", "done"}

\* No spawn can reach a closed scope: a task that has not finished holds
\* only open scopes (so the v0 panic P0005 stays unreachable).
SpawnOnlyIntoOpenScopes ==
  \A t \in TaskIds : st[t] # "done" => \A s \in scopes[t] : sst[s] = "open"

\* At most one running task per worker.
WithinWorkers == Busy <= Workers

\* No task holds its own handle, so no task can join itself.
NoSelfJoin == \A t \in TaskIds : t \notin knows[t]

\* No cycle of waits. A wait is a join or the end of a scope (which waits
\* for each task of the scope). With TLC's deadlock check (on by default)
\* this says the scheduler never deadlocks.
WaitsFor(t) ==
  CASE st[t] = "joining" -> {waits[t]}
    [] st[t] = "closing" -> {m \in Members(waits[t]) : st[m] # "done"}
    [] OTHER -> {}
RECURSIVE Reach(_, _)
Reach(frontier, n) ==
  IF n = 0 THEN frontier
  ELSE Reach(frontier \cup UNION {WaitsFor(t) : t \in frontier}, n - 1)
NoWaitCycle ==
  \A t \in TaskIds : t \notin Reach(WaitsFor(t), MaxTasks)

\* Every program ends: `main` returns. With MainEndsLast this says every
\* task ends, so every join returns and every scope's end stops waiting.
ProgramEnds == <>(st[Main] = "done")
=============================================================================
