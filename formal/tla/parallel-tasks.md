# Parallel tasks and shared reference counts: design notes

Design notes for the parallel-execution milestone's spec (ADR 0019, issue #432). TLC checks the models in this directory in the `formal` step of `scripts/check.sh`. Today's sequential tasks are described in `docs/design.md`, "Concurrency in v0". This note covers what the parallel design must keep, what the models check, and what they found.

## The models

| Model | What it covers | Bound | TLC in the formal step |
| --- | --- | --- | --- |
| `ScopedTasks` | the scheduler, scopes, `spawn`, `join` | `main` and 3 spawned tasks, 3 scopes, 2 workers | about 50 s (241,480 states), mostly the temporal property |
| `SharedCounts` | reference counts on values that tasks share, and the `var` write rule (ADR 0001) | 3 tasks that start out sharing a value, 3 value ids, 2 references per task per value, 2 writes | about 6 s (56,784 states) |
| `SharedCountsRacy` | `SharedCounts` with a count update that is not atomic (broken on purpose) | as `SharedCounts` | fails in about 2 s: `NoUseAfterFree` violated after 8 steps |
| `ScopedTasksLeakyScopes` | `ScopedTasks` without the scope-capture rule below (broken on purpose) | as `ScopedTasks` | fails in about 1 s: `SpawnOnlyIntoOpenScopes` violated after 5 steps |

Times are wall-clock on a 4-core Linux machine, with TLC's single worker as `scripts/formal/run.sh` runs it. The whole TLC part of the step takes about a minute.

A model whose `.cfg` starts with `\* expect-violation: NAME` is broken on purpose. The step passes it only when TLC fails it by finding invariant `NAME` violated (`formal_tlc_verdict` in `scripts/formal/pins.sh`). So a change that makes `SharedCountsRacy` pass, which would mean the counts model can no longer fail, fails the step. `scripts/formal/test.sh` tests that rule without TLC.

The models check sequentially consistent interleavings: TLC runs one atomic step at a time. The memory orderings below are argued from that, not model-checked.

## Properties

| Property | Model | Kind | Says |
| --- | --- | --- | --- |
| `NoUseAfterFree` | SharedCounts | invariant | every value a task holds is live |
| `NoVisibleInPlaceChange` | SharedCounts | invariant | every task sees in each value it holds the contents it last saw there |
| `CountIsReferences` | SharedCounts | invariant | a live value's count is the number of references held, and is above 0; a freed value has none (no early free, no leak) |
| `ScopeEndsAfterItsTasks` | ScopedTasks | invariant | a closed scope has no task left to finish |
| `TaskEndsAfterItsScopes`, `MainEndsLast` | ScopedTasks | invariant | a task returns only after its scopes close, so `main` returns last |
| `SpawnOnlyIntoOpenScopes` | ScopedTasks | invariant | an unfinished task holds only open scopes, so `spawn` never reaches a closed one (`P0005`) |
| `WithinWorkers` | ScopedTasks | invariant | no more tasks run than there are workers |
| `NoSelfJoin`, `NoWaitCycle` | ScopedTasks | invariant | no task holds its own handle; no cycle of joins and scope ends |
| deadlock check | both | TLC's own (on) | no state but the end has no next step |
| `ProgramEnds` | ScopedTasks | temporal, under fairness | `main` returns, so (with `MainEndsLast`) every join returns and every scope's end stops waiting |

## The scheduler

- A fixed pool of workers (OS threads) runs ready tasks. `spawn` makes a task ready, and any free worker may start it.
- A task that waits **parks**: it gives up its worker until it may go on. A task waits in two places only:
  - `h.join()` on a task that has not finished;
  - the end of a `scope` block while a task of that scope still runs.
- A worker never runs a task inside another task. The task that finishes wakes the tasks that joined it, and the last task of a scope wakes the scope's owner.
- Because waiting gives the worker back, a fixed pool cannot run out of workers while every task waits for a task that has no worker. If join blocked the OS thread instead, 2 workers each joining a task still in the queue would deadlock. The spec must keep "waiting parks", or grow the pool.
- The scheduler must start every ready task eventually, even while other tasks keep taking the free workers. The model asks for strong fairness on scheduling. At this bound weak fairness passes too (checked once by hand), so TLC cannot tell the two apart here. The spec should still ask for the strong form, which a FIFO run queue gives.
- A join may also run the joined task itself on the joiner's worker when that task has not started, because the joiner would only wait for it. It must never run any other task there. Both the joiner and a free worker could then try to start that task, so starting it needs an atomic claim (for example, a compare-and-swap of its state from queued to running), and only the one that wins runs it. `ScopedTasks` models parking, and does not model this shortcut or its claim.

### Why join and scope order rule out #482

Issue #482 is a sequential-v0 bug: `join` runs the queued tasks of the scope, oldest first, up to the joined one. So `x.join()` inside task `a` runs task `b`, which joins `a` while `a` is still on the stack. In the parallel design, `join` parks the joiner and starts nothing on its stack, so `b` runs as a task of its own, and `b`'s join of `a` waits until `a` returns. That program is in `ScopedTasks`' state space (`main`, `a`, `x`, `b`, one scope), and every interleaving of it ends.

No join can close a cycle, for these reasons:

- A handle exists only once `spawn` returns, so a task can capture only the handles of tasks spawned before it. It never holds its own handle (`NoSelfJoin`).
- A task gets other handles only by spawning a task, or from the result of a task that has finished. A finished task is not waiting, so it cannot be part of a cycle.
- The end of a scope waits only for the scope's own tasks, which start inside the owner's block. For one of them to hold the owner's handle, that handle would have to enter the owner's block, and a value enters a running block only from its captures or from a finished task's result.

TLC checks `NoWaitCycle` and the deadlock check for every program up to the bound. Here every spawned task captures every handle its spawner holds, and every join returns every handle the joined task holds, which allows the most joins. A one-off run at a larger bound (`main` and 4 spawned tasks, 2 scopes: 1,026,453 states, 16 s with 3 TLC workers) also passed. The bullets above are an argument, not a proof, for programs beyond the bound.

**Deadlocks a program can see:** none in the model. Oska Core has no channels, locks or `Sched` yet, so a task waits only on `join` and on the end of a scope, and neither can deadlock. Channels and `select` will add deadlocks that the program itself causes; they need their own model (ADR 0019).

### Scope handles must not outlive their block inside a task (finding)

`ScopedTasksLeakyScopes` lets a task spawned on scope `outer` capture the handle of a scope `inner` opened inside `outer`'s block. TLC finds a spawn on `inner` after `inner`'s block has ended, which is the v0 panic `P0005`. Today's checker accepts such a program, since `E0292` lets `spawn` take a scoped closure. At 11876b9 this program stops with `P0005` (exit 101) under `oskac run`:

```oska
fn main() {
    let v = scope |outer| {
        scope |inner| {
            let h = outer.spawn(|| {
                let t = inner.spawn(|| 1)
                t.join()
            })
            0
        }
    }
    let _ = v
}
```

`docs/runtime.md` says no checked program can reach `P0005`, so today this is a bug of its own, apart from parallelism ([#484](https://github.com/aparsons/oska/issues/484)). With parallel tasks, the task could also try to spawn into `inner` while `inner`'s owner is about to close it. The spec needs the rule that `ScopedTasks` checks with `ConfineScopes`: **a closure spawned on scope `s` may hold only the handles of `s` and of the scopes whose blocks enclose `s`'s block.** The checker should enforce it (extending `E0292`). `P0005` then stays a second line of defence that no checked program reaches.

## Reference counts across tasks

ADR 0001: values are reference counted and immutable when shared; a `var` is changed in place when its count is 1, and copied first otherwise. Tasks share values freely. A value crosses to another task when a spawned task captures it and when `join` returns it, so its count changes on more than one task.

### What the count operations must be

1. **Share** (a capture at `spawn`, a result at `join`, `let y = x`) is one atomic increment. Only a task that holds a reference may share it. So a count goes up only while it is at least 1, and a freed value is never brought back.
2. **Drop** is one atomic decrement. The task whose decrement takes the count from 1 to 0 frees the value.
3. **Write through a `var`** loads the count once. If it is 1, it changes the value in place. Otherwise it copies the value into a new one with count 1, changes the copy, and drops its reference to the original (step 2).

`SharedCounts` checks this design with each count update as one atomic step. The check before a write is a plain load, separate from the write, as an implementation does it. TLC finds no violation of `NoUseAfterFree`, `NoVisibleInPlaceChange` or `CountIsReferences`.

### Why the update must be atomic (`SharedCountsRacy`)

With each update as a load and then a separate store, TLC finds a value freed while a task still holds it. A share loads the count, a drop of the same value loads the same count, and the share's store is lost when the drop stores its own. The count is then lower than the references held, and the last drop it counts frees a value another task still holds. The same lost update can let a write see a count of 1 while another task holds the value, which breaks `NoVisibleInPlaceChange`. So a count that two tasks can update needs an atomic read-modify-write: a hardware fetch-add or fetch-sub, not a load and a store.

### Why the check needs no compare-and-swap

The write rule is a load, then a write. Another task could raise the count between the two only if it held a reference, and a count of 1 means the writer holds the only reference. So nothing can change the count between the check and the write, and TLC confirms it with the two as separate steps. This rests on rule 1: **only a holder can make a new reference.** If Oska ever adds weak references, a cache or interning table that hands out references, or anything else that makes a reference without holding one, the check must become a compare-and-swap (or that table must hold a counted reference).

### Memory orderings

These follow Rust's `Arc`, which uses the same scheme. They come from the C++/Rust memory model and are not model-checked here.

| Operation | Ordering | Why |
| --- | --- | --- |
| share (increment) | `Relaxed` fetch-add | the sharer holds a reference, so the value cannot be freed meanwhile; the new reference reaches the other task through `spawn` or `join`, which synchronize already |
| drop (decrement) | `Release` fetch-sub | this task's reads of the value happen before whoever frees it |
| free, when the decrement returned 1 | `Acquire` fence before freeing | every other task's reads, released by its own decrement, happen before the free |
| check before an in-place write | `Acquire` load | a task that dropped its reference has finished reading the value before it is changed; without it a weak memory model allows that task to read the changed value |
| `spawn` and `join` | `Release` on publish, `Acquire` on take (the run queue and the task's completion) | the contents of a value, and the counts written before it crossed, are visible to the task that gets it |

- A count overflow must stop the program, not wrap, as `Arc` does. A wrapped count would free a live value.
- A new value's count (1) is a plain store. No other task can see the value until `spawn` or `join` publishes it.

### Cost, and what the models leave open

- Every count update on a value that tasks may share is an atomic instruction. A value that never leaves its task could use plain counts, but telling the two apart is a design of its own (for example, biased reference counting). It would need its own model before it is built.
- A task that panics finishes as far as waits go. The model treats it as returning, so joiners and the scope's owner wake. The spec must say what `join` returns from a task that panicked.
- The two models are separate to keep TLC fast. `SharedCounts` lets any task share with any other at any time, which covers every way `ScopedTasks` passes values between tasks.
- The models do not check `Sched` channels or `select` (ADR 0019 requires their own models first), memory orderings, or programs beyond the bound.
