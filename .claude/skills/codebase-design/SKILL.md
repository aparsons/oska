---
name: codebase-design
description: Vocabulary and principles for deep modules with small interfaces at clear seams. Use to design or improve a module interface, find deepening opportunities, place a seam, or make code easier to test or for agents to navigate.
---

# Codebase design

This skill gives shared words and principles for module design. The goal is a deep module: much behaviour behind a small interface, at a clear seam, tested through that interface. The `tdd` and `improve-codebase-architecture` skills use these words. Use them exactly.

## Glossary

| Term | Meaning | Avoid |
|---|---|---|
| **Module** | Anything with an interface and an implementation, at any scale: a function, a type, a crate, a service. | component, unit (as a synonym) |
| **Interface** | Everything a caller must know to use the module: types, invariants, call order, errors, configuration, and performance. | API (when it means only signatures), contract |
| **Implementation** | The code behind the interface that callers do not need to know. | internals (as a loose synonym) |
| **Depth** | The behaviour a caller gets for each unit of interface that the caller learns. | size, line count |
| **Seam** | A place where you can change behaviour without editing the code at that place. The interface lives at the seam. The term comes from Michael Feathers. | boundary, layer |
| **Adapter** | A concrete thing that fills a seam, such as a real host or a fake host. | plugin, driver (as synonyms) |
| **Leverage** | What callers gain from depth: more capability for less learning. | power, reuse |
| **Locality** | What maintainers gain from depth: a change or a bug stays in one place. | cohesion (as a synonym) |

## Deep and shallow

```
Deep module                    Shallow module
+------------------+           +------------------------------+
|  small interface |           |  large interface             |
+------------------+           +------------------------------+
|                  |           |  little behaviour            |
|  much behaviour  |           +------------------------------+
|                  |
+------------------+
```

To make a module deeper, ask these questions:

- Can the module have fewer entry points?
- Can the parameters be simpler?
- Can more of the work move inside, out of the callers?

## Principles

- **Depth is a property of the interface.** Measure it from the caller side. A long implementation is not deep if the interface is just as complex.
- **Use the deletion test.** Imagine that you delete the module. If its complexity goes away, the module was shallow. If the complexity spreads to every caller, the module earns its place.
- **The interface is the test surface.** Test at the interface that callers use. Then a refactor behind it does not break the tests.
- **One adapter is a guess. Two adapters make a seam real.** A seam with only one adapter is only indirection. Add the seam when a second adapter exists, for example a fake for tests.

## Testability

Pass dependencies in. Return results, not side effects. Keep the surface small.

```rust
// Hard to test: reads the clock and writes the file itself.
fn record_visit(path: &Path) -> io::Result<()> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    fs::write(path, format!("{secs}\n"))
}

// Easy to test: takes the time, returns the line. The caller writes it.
fn visit_line(now_secs: u64) -> String {
    format!("{now_secs}\n")
}
```

```rust
// The dependency comes in as a trait. RealHost and FakeHost both fill it.
// (Shape of oska_run::run_program.)
pub fn run_program(program: &Program, host: &mut dyn Host, args: &[String]) -> RunOutcome { /* ... */ }
```

## In Oska

The repo is Oska when `crates/oskac/` and `scripts/check.sh` exist. Use these real examples when you apply the terms.

- **Crates are the largest modules.** The graph is: `oska-syntax` and `oska-sys` depend on no workspace crate; `oska-check` and `oska-eval` use `oska-syntax`; `oska-native` uses `oska-check` and `oska-syntax`; `oska-run` uses `oska-check`, `oska-native`, `oska-syntax`, and `oska-sys`; `oskac` uses all of them. A new edge widens an interface. Ask for a reason first.
- **A crate's interface is its `lib.rs` re-exports.** Keep inner modules `pub(crate)` by default. Integration tests under `tests/` use only the public interface.
- **The `Host` trait is the main seam.** It lives in `crates/oska-run/src/host.rs`. `RealHost` and `FakeHost` are its two adapters, so the seam is real. A new effect goes on `Host` and gets both adapters. `CODING_STANDARDS.md` ("Grow Oska") lists the full path.
- **Capability attenuation is a seam in Oska programs.** A function that gets a directory capability can hand a narrower one to a callee. The callee's interface then shows exactly the authority that it needs. `GLOSSARY.md` defines the terms.
- **`oska-sys` is deep on purpose.** It hides all `unsafe` behind a small safe interface (ADR 0009). Do not move `unsafe` out of it.
- **`std.rt` is the only place for system calls** in Oska code (ADR 0013).

Read `GLOSSARY.md` and the ADRs in `docs/adr/` before you propose a change to a crate interface. If a proposal contradicts an ADR, say so.

## How the terms relate

- A module has one interface and one implementation.
- Depth compares the behaviour of a module with the size of its interface.
- An interface sits at a seam.
- An adapter fills a seam.
- Depth gives leverage to callers and locality to maintainers.
- Tests use the interface, so tests sit at seams.

## Rejected framings

- **Depth as a ratio of lines.** Line counts do not measure what a caller must learn.
- **"Interface" as only a language keyword.** The interface includes errors, order, and performance, not only a `trait` or an `interface` declaration.
- **"Boundary".** The word has too many meanings. Say "seam" or "interface".

## Reference files

- Read [DEEPENING.md](DEEPENING.md) when you deepen a module and must decide how to test it.
- Read [DESIGN-TWICE.md](DESIGN-TWICE.md) when you want to compare alternative interfaces before you choose one.

## Steps

1. **Name the module and its callers.** Write the module, its current interface, and its main callers, with glossary terms.
   Done when: the module and each caller are named.

2. **Apply the deletion test and the depth questions.** Write the result for this module.
   Done when: you wrote whether the module is deep or shallow, and why.

3. **Place the seam.** Choose where the interface goes. Read `DEEPENING.md` to classify each dependency.
   Done when: each dependency has a category and a test plan.

4. **Compare alternatives if the choice is open.** Follow `DESIGN-TWICE.md`.
   Done when: you gave a firm recommendation, or the user chose a design.

## Security

- **Trust boundaries go at seams.** Validate untrusted input once, at the seam where it enters. Give inner code typed, validated values.
- **Hide secrets.** A deep module keeps its secrets and credentials hidden from callers. Callers never receive a raw credential.
- **Authority is part of the interface.** A seam that passes a capability passes authority. Pass the narrowest capability that does the job. A new crate edge to `oska-sys` widens access to system calls, so it needs an explicit reason.
