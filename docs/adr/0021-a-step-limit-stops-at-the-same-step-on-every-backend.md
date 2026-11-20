# A step limit stops at the same step on every backend

## Decision

An **embedder** that runs code a model wrote needs to bound how long that code runs. `oskac run` will take a step limit for this. A **step** is one function call or one loop back-edge (the jump to the next iteration of a loop). The interpreter and the native backend count steps the same way, so a program under a step limit stops at the same step, with the same output, on both. A program that reaches its limit panics with its own stable code and exits with 101, as every panic does.

A step limit is a separate limit from a wall-clock timeout. A timeout is not deterministic, and the differential suite does not compare where it stops a program.

## Why

The interpreter is the reference for what every program does, and the differential suite fails on any difference in output, exit code or panic between it and the native backend ([ADR 0007](0007-interpreter-runs-the-checked-program.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)). A limit that stops the two backends at different points breaks that rule for every program that reaches it. An embedder also needs a limit it can test: the same snippet with the same limit must give the same result on every run and every platform.

## Considered options

- **The native limit is a wall-clock time only; steps work only with `--interpret`.** Rejected: it is cheaper for native code, but the same `--max-steps` would mean nothing for an executable, and output under a limit would differ between backends.
- **Count every instruction or expression.** Rejected: the count would depend on how each backend lowers code. Calls and back-edges exist in the checked program, so both backends can find the same ones.

## Consequences

The native backend emits a counter at each call and each loop back-edge, which costs time in a loop. The spec for run limits decides whether the counter is present when no step limit is set. Calls inside the runtime (`std.rt`) are not steps; calls to `std` functions written in Oska are. The differential suite gains programs that reach the limit.
