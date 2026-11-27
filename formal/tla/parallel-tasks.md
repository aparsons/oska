# Parallel tasks, shared reference counts and channels: design notes

Design notes for the parallel-execution milestone's spec (ADR 0019, issues #432 and #433). TLC checks the models in this directory in the `formal` step of `scripts/check.sh`. Today's sequential tasks are described in `docs/design.md`, "Concurrency in v0". This note covers what the parallel design must keep, what the models check, and what they found. `Sched` channels and `select` are in [Channels and select](#channels-and-select).

## The models

| Model | What it covers | Bound | TLC in the formal step |
| --- | --- | --- | --- |
| `ScopedTasks` | the scheduler, scopes, `spawn`, `join` | `main` and 3 spawned tasks, 3 scopes, 2 workers | about 50 s (241,480 states), mostly the temporal property |
| `SharedCounts` | reference counts on values that tasks share, and the `var` write rule (ADR 0001) | 3 tasks that start out sharing a value, 3 value ids, 2 references per task per value, 2 writes | about 6 s (56,784 states) |
| `SharedCountsRacy` | `SharedCounts` with a count update that is not atomic (broken on purpose) | as `SharedCounts` | fails in about 2 s: `NoUseAfterFree` violated after 8 steps |
| `ScopedTasksLeakyScopes` | `ScopedTasks` without the scope-capture rule below (broken on purpose) | as `ScopedTasks` | fails in about 1 s: `SpawnOnlyIntoOpenScopes` violated after 5 steps |
| `Channels` | `Sched` channels: send, receive, close, `select`, and the deadlocks they allow | `main` and 2 spawned tasks, 2 channels (a rendezvous, and one that holds 1 message), 2 messages, 2 workers | about 40 s (172,310 states; 38 s to 46 s measured), 5 s of it on the temporal property |
| `ChannelsBuffer2` | `Channels` with a buffer of 2, so a buffer holds two messages and their order is checked | as `Channels`, but the second channel holds 2 messages | about 36 s (168,369 states) |
| `ChannelsRacySelect` | `Channels` with a `select` that scans its arms without locking them (broken on purpose) | as `Channels` | fails in about 1 s: `SelectChoosesReady` violated in a 6-state trace |
| `ChannelsLossyClose` | `Channels` with a close that drops buffered messages (broken on purpose) | as `Channels` | fails in about 1 s: `NoLostMessage` violated in a 4-state trace |

Times are wall-clock on a 4-core Linux machine, with TLC's single worker as `scripts/formal/run.sh` runs it. The whole TLC part of the step takes about two and a half minutes.

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
| deadlock check | ScopedTasks, SharedCounts | TLC's own (on) | no state but the end has no next step |
| `ProgramEnds` | ScopedTasks | temporal, under fairness | `main` returns, so (with `MainEndsLast`) every join returns and every scope's end stops waiting |
| `NoLostMessage`, `NoDuplicateMessage` | Channels | invariant | every message sent is in exactly one place: buffered, held by its parked sender, received once, or returned to its sender by a failed send |
| `FifoOrder` | Channels | invariant | a channel's buffer is oldest first, and its parked senders hold newer messages than it |
| `ReceivedInOrder` | Channels | invariant | every message still waiting on a channel is newer than the newest one received from it, so receives follow send order |
| `ClosedRefusesSends` | Channels | invariant | every send that starts after the close returns its message, and no sender stays parked on a closed channel |
| `EndOnlyWhenDrained` | Channels | invariant | a receive returns `None`, and a `select` "all closed", only for channels that are closed and drained |
| `SelectChoosesReady` | Channels | invariant | the arm a `select` picked still has a message when it takes it |
| `ParkedOnlyWhileBlocked` | Channels | invariant | no lost wake-up: a task parked on a channel waits for something that has not happened yet |
| `WithinWorkers` | Channels | invariant | as in ScopedTasks |
| `DeadlockNeedsAChannel`, `DeadlockIsReal` | Channels | invariant | a reported deadlock has a task parked on a channel, and no task can take any step again (stated with `ENABLED`, apart from the runtime's test) |
| deadlock check | Channels | TLC's own (on) | every state but the end has a next step: a program that deadlocks reaches the runtime's report, never a silent hang |
| `EndsOrReportsDeadlock` | Channels | temporal, under fairness | the program ends, or the runtime reports its deadlock |

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

**Deadlocks a program can see:** none in this model. Without channels a task waits only on `join` and on the end of a scope, and neither can deadlock. Channels add deadlocks that the program itself causes; `Channels` models them (see [Deadlock](#deadlock)).

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
- `ScopedTasks` and `SharedCounts` are separate to keep TLC fast. `SharedCounts` lets any task share with any other at any time, which covers every way `ScopedTasks` passes values between tasks.
- The models do not check memory orderings, or programs beyond the bound.

## Floats share no state between tasks (ADR 0027)

`Float` adds no model. A `Float` is 64 bits held in a value slot, with no heap cell, no reference count and no interior mutability (ADR 0001, ADR 0027). A float operation reads its operands and returns a new value: there is no rounding-mode state, no sticky flag and no fast-math switch, so two tasks cannot see each other through the floating-point unit. A `Float` that crosses a task (a spawn argument, a result or a channel message) is copied like an `Int`, and a `List<Float>` is a list like any other, so its count is the one `SharedCounts` already models. `Secret<Float>` does not exist (`E0357`), so no float is held under a constant-time promise. The Lean model of `Float` (`formal/lean/OskaFormal/Float/`) is a function from bits to bits for that reason. A future `Float` feature that adds shared state, such as a per-task rounding mode, needs a model here first (ADR 0019).

## Channels and select

`Sched` channels and `select` let tasks pass messages, and they are nondeterministic, so they need `Sched` (`docs/design.md`, Concurrency). `Channels` checks the semantics below. The surface syntax is not settled; `tx.send(v)`, `rx.recv()`, `tx.close()` and `select` here name operations, not final spellings.

The model's scheduler is a copy of `ScopedTasks`' (a fixed pool of workers; a task that waits parks and gives its worker back), not an `EXTENDS`: `ScopedTasks`' open-ended scopes, spawns and joins together with channels would not finish in CI. A change to `ScopedTasks`' scheduler must be mirrored in `Channels`. The model fixes the task tree to keep TLC fast: `main` opens one scope, spawns the other tasks into it at the start, and its last step ends the scope, which waits for them. `ScopedTasks` checks spawn, join and scopes in general. Within the bound every program is in the state space: every task holds every channel, and at any step a running task may send on, receive from or close any channel, `select` over both channels, or return. Each send makes a new message id, so TLC can follow every message.

### Semantics

| Operation | When | Result |
| --- | --- | --- |
| make a channel | capacity `n >= 0`, fixed at creation | `n = 0` is a rendezvous: a send returns only once a receiver has taken its message. `Channels` checks capacities 0 and 1, and `ChannelsBuffer2` capacities 0 and 2. There are no unbounded channels: a sender that never waits can use memory without bound. |
| `tx.send(v)` | a receive or `select` is parked on the channel | hands `v` to one of them, which wakes with it; returns `Ok(())` |
| | the buffer has room | buffers `v`; returns `Ok(())` |
| | the buffer is full (always, at capacity 0) | parks until a receiver takes `v` (`Ok(())`) or the channel closes (`Err(v)`) |
| | the channel is closed | returns `Err(v)` at once: the sender keeps its message. No panic. |
| `rx.recv()` | a message is buffered or a sender is parked | `Some(oldest message)`; the oldest parked sender's message moves into the room made, and that sender wakes with `Ok(())` |
| | the channel is open and empty | parks until a message comes (`Some`) or the channel closes (`None`) |
| | the channel is closed and drained | `None`, every time |
| `tx.close()` | open | closes it. Buffered messages stay, and receives get them before `None`. Parked senders wake with `Err(v)`. A parked receive wakes with `None`; a parked `select` wakes with "all closed" once every arm is drained. |
| | already closed | does nothing, so two tasks may race to close. The model does not check this case: its `Close` step is disabled on a closed channel, which is the same as doing nothing. |
| `select` | some arm has a message | takes the oldest message of one such arm, as `recv` would |
| | no arm has a message, every arm is closed and drained | returns "all closed" |
| | otherwise | parks on all the arms; the first message sent on any of them wakes it with that message, and the last arm to drain wakes it with "all closed" |

- **Order.** A channel is first in, first out. Messages are received from a channel in the order their sends started, across all senders (`ReceivedInOrder`), so a task's messages on one channel arrive in the order it sent them. Its buffer is oldest first and parked senders are served oldest first (`FifoOrder`). Three ways to break the order each fail TLC: adding to the front of the buffer (`ChannelsBuffer2`), serving the newest parked sender first, and taking a parked sender's message before the buffer's head (`Channels`). A buffer of 1 cannot hold two messages and a buffer of 2 needs 3 messages to be full with a sender parked, so each takes the configuration named.
- **No message is lost or duplicated** (`NoLostMessage`, `NoDuplicateMessage`): every message is, at every moment, in exactly one place: buffered, held by its parked sender, received by exactly one receive, or back with its sender after `Err(v)`. A message still buffered when the program drops the channel's last reference is dropped with it, as any value is; no receive was told it arrived.
- **Closing** (`ClosedRefusesSends`, `EndOnlyWhenDrained`): every send that starts after the close fails with `Err(v)`; a closed channel keeps no parked sender; and `None` (or "all closed") means the channel is closed and every message it accepted has been received.
- **`select` has receive arms only, at least two.** A send arm, a timeout (which needs `Clock`) and a default arm are not in this design; adding one extends `Channels` first (ADR 0019). The model lets `select` take any arm that has a message. The spec should pick among them uniformly at random, so no arm starves; TLC does not check that.
- **Who may close.** The model lets any task that holds the channel close it. If the API splits a channel into ends, the spec should let only the sending end close.

### Atomicity

- Each `send`, `recv` and `close` is one atomic step under the channel's lock. In particular a send to a channel with a parked receiver hands the message over and wakes the receiver in the same step, so no wake-up is lost: `ParkedOnlyWhileBlocked` says that a parked task always waits for something that has not happened yet.
- A `select` scans its arms, picks one that has a message, then takes it. It must hold the locks of **all** its arms from the scan to the take, or do both in one step under them. To lock several channels without a lock-order deadlock, it takes them in one global order, for example by the channel's address; the model takes them in one step, which that order makes safe.
- **Why (`ChannelsRacySelect`).** Without the locks, TLC finds in 6 states: `main` sends a message on channel 2, `select`s, and picks channel 2; task 1 receives that message from channel 2; `main`'s select now takes from an empty channel. The implementation would return garbage, or a spurious "closed", or must scan again.
- **Why close keeps the buffer (`ChannelsLossyClose`).** With a close that empties the buffer, TLC finds in 4 states a message whose send returned `Ok(())` and that no receive can ever get.

### Deadlock

Joins and scopes alone never deadlock (`ScopedTasks`). Channels let a program deadlock itself, and TLC reaches each of these within the bound:

- a receive, or a `select`, on channels that no running task will send on or close;
- a send on a full or rendezvous channel that no running task will receive from. TLC's shortest deadlock (8 states): `main` and task 1 each send on the rendezvous channel, and task 2 receives on the other channel;
- the end of a scope, or a join, waiting for a task that is parked on a channel only the waiting task would serve: for example `main` ends its scope while a task is parked sending to a channel that `main` was to drain. Expect this one most;
- tasks that each receive from a channel only another of them sends on.

Every deadlock has a task parked on a channel (`DeadlockNeedsAChannel`).

**How a program sees it: the runtime stops it with a deadlock panic.** The scheduler counts the tasks that are ready, running, or in a host call. When the last of them parks and `main` has not returned, every task is parked, and none can wake: a task wakes only by another task's step, and `ParkedOnlyWhileBlocked` says no wake is pending. The runtime then panics (exit 101, a new `P` code chosen with the implementation) and names each parked task and what it waits on. Like every panic, it cannot be caught.

- **Exact.** When the report comes, no task can take a step again: no worker can start a task, no operation is enabled, and `main` cannot wake (`DeadlockIsReal`, stated with `ENABLED`, apart from the runtime's all-parked test). So there is no false report, and `ParkedOnlyWhileBlocked` explains why the all-parked test suffices. TLC's deadlock check finds no state without a next step but the end, and `EndsOrReportsDeadlock` holds under fairness, so a deadlocked program never hangs silently.
- **Partial deadlocks.** While any task runs, the runtime cannot know whether it will free the parked ones, so it reports nothing. Every task belongs to a scope whose owner waits for it, so a stuck task in time holds up its scope's owner, and so `main`. Once every other task has finished or parked, the deadlock is total and reported. A task that loops forever keeps it hidden, as in a sequential program.
- **Host calls.** A task blocked in a host call (reading `Stdin`, a socket, sleeping on `Clock`) is not parked: a host event can wake it. The count must include it, or the runtime would report a deadlock that input could end. The model has no host calls.

### What `Channels` leaves open

- Closing when the last sending end is dropped. ADR 0001's counts could do it, and it would turn the first kind of deadlock into a `None`. The model has explicit `close` only; drop-closing extends the model first.
- Fairness among `select`'s arms, send arms, timeouts, and non-blocking `try_send`/`try_recv`.
- Joins between the tasks other than the scope's end; `ScopedTasks` checks those. A join waits as the scope's end does, so it adds no kind of deadlock beyond the third above.
- The message values' reference counts. A send moves the sender's reference into the channel and a receive moves it out, so they are the moves `SharedCounts` already allows.
- Programs beyond the bound. A one-off run at 3 messages (728,602 states, 82 s with 3 TLC workers) also passed.

### Open questions for the maintainer

The model makes these product choices. The milestone's spec must confirm each, or change it and the model with it.

- **Bounded channels only.** Every channel has a capacity fixed at creation, `0` included. Unbounded channels are left out.
- **A deadlock panic stops the whole program and names every parked task.** This differs from `docs/design.md` (its list of exceptions says "Panics stop the current task") and from a panic's single span. It needs a new `P` code, and the spec must say how the report shows more than one task.
- **Who may close.** The model lets any task that holds the channel close it. The recommendation is that only the sending end may close it.
- **Send on a closed channel returns `Err(v)`, and closing twice does nothing**, as Rust's `mpsc` does, not a panic as in Go.
- **`select` fairness.** The notes recommend choosing uniformly at random among the ready arms. TLC does not check it: the model lets `select` take any ready arm.
