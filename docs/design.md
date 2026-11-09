# Oska design notes

Working draft. Everything here is open to change.

Background research: [`research/2026-11-ai-first-secure-language.md`](research/2026-11-ai-first-secure-language.md).

## Principles

1. **Secure by default, unsafe by explicit request.** The safe path is the shortest path. Anything dangerous is opt-in, named, and grep-able.
2. **No ambient authority.** I/O, network, filesystem, environment, clock, randomness, and process spawning are *capabilities*: values passed as arguments. A function without a capability parameter cannot perform that effect.
3. **Legible to humans and models.** A function signature should tell you everything it can do. One obvious way to write things; no context-sensitive syntax.
4. **Machine-first tooling.** Every diagnostic has a stable code, a precise span, and (where possible) a machine-applicable fix, emitted as JSON with `--format=json`.

## Safety baseline

- Memory safety (no raw pointers in safe code).
- No null: use `Option<T>`.
- Errors as values: `Result<T, E>`; no unchecked exceptions.
- Checked integer arithmetic by default; wrapping and saturating are explicit operations.
- No implicit numeric conversions.
- Immutable bindings by default (`let`); mutation via `var`.
- No global mutable state. Module-level items are constants, functions, and types only. Globals are ambient authority.
- Untrusted input is tainted (`Untrusted<T>`) until it is validated, so it cannot reach sinks such as SQL, shell, or HTML without a validation step.

## Capabilities

```oska
fn read_config(fs: FsRead, path: Path) -> Result<Config, ConfigError> {
    let text = fs.read_to_string(path)?
    Config.parse(text)
}
```

### Authority hierarchy

```oska
fn main(fs: Fs, net: Net) -> Result<(), AppError> {
    let config_fs = fs.restrict_to("/etc/myapp").read_only()
    let config = read_config(config_fs, Path.new("config.toml"))?
    serve(net.connect_only(), config)
}
```

- `main` is the only root of authority. It declares the capabilities it needs, and the runtime grants exactly those to `main` and to nothing else. A program's whole authority can be read from that one signature.
- Capabilities cannot be forged. There is no constructor that makes one out of nothing. You can get a capability in only two ways:
  - deriving it from a capability you already hold
  - being passed one
- If code has no value of the right capability type, it cannot perform the effect, and the compiler rejects it.
- Authority only flows downward. Derivation narrows a capability and never widens it:
  - `Fs → FsRead`
  - `Fs.restrict_to(dir)`
  - `Net → NetConnect`
  - There is no way to turn a narrower capability back into a broader one.
- Capabilities are ordinary values, so they show up in function signatures. A signature without capability parameters describes a function that cannot perform effects. This is how principle 3 holds for effects.
- Dependencies can only use capabilities you pass them, which limits supply-chain blast radius.

## Injection-proof sinks

`Untrusted<T>` stops unvalidated data from reaching a sink. Sink APIs go further: they are designed so that data cannot be confused with code, even when the data has been validated.

| Sink | Safe API shape | Not provided |
| --- | --- | --- |
| SQL | Query text must be a compile-time literal. Values bind through typed parameters: `db.query(sql"SELECT * FROM users WHERE id = ?", id)`. | Building queries by string concatenation or interpolation. |
| Process | The program and arguments are separate values: `proc.run("git", ["log", branch])`. | A shell string. Shell invocation is a separate, explicitly named capability. |
| HTML | Templates escape by default for their context. Raw output requires a `TrustedHtml` value. | Writing untyped strings into markup. |
| Paths | `Fs.restrict_to` confines access to a directory. Joining a path that escapes the root is an error. | `..` traversal outside a restricted capability. |
| Logging | Structured fields. Secret-typed values (`Secret<T>`) are redacted. | Interpolating raw strings into log lines. |

Building a query string by hand is a compile error with a stable diagnostic code, not a lint.

## Unsafe and FFI

Escape hatches are where memory-safety and capability guarantees break. They need to be rare, visible, and contained.

- `unsafe { ... }` blocks and `extern` (FFI) declarations are the only places the safety baseline can be suspended.
- Each package declares in its manifest whether it uses `unsafe` or FFI. Packages that do not declare it are rejected if they contain either.
- The application manifest allow-lists which dependencies may use `unsafe` or FFI. This is modeled on Pony's `--safe`.
- FFI functions must declare the capabilities they need in their Oska signature. Callers then have to supply those capabilities, so foreign code cannot reach the outside world without going through the capability system.
- `oskac audit` lists every `unsafe` and FFI site in the dependency graph as JSON, so agents and reviewers can review them.

## AI-first features (to explore)

- Contracts (`requires` / `ensures`) used as machine-checkable specs, so that agent-written code only has to be *checkable*, not right the first time.
  - Contracts are checked in all builds, not just debug builds.
  - The compiler proves decidable contracts statically, using an SMT solver.
  - Everything else becomes a runtime check.
  - The compiler can always report which contracts were proved and which are runtime-checked.
- Structured diagnostics and a stable, documented AST/JSON export for agents.
- `intent` doc blocks the compiler can cross-check against signatures (for example, flagging "pure" functions that take capabilities).
- Deterministic formatting with a single canonical style.

## Open questions

- Memory management: ownership/borrowing vs. region-based vs. GC with value types.
- Compilation target: LLVM, Cranelift, or WASM first.
- Concurrency model and how capabilities flow across tasks.
- Should contracts be mandatory, for example on `pub` functions or on functions that take `Untrusted<T>`? And how much SMT power is worth the compile time and the difficulty for models to learn?
- Effect granularity: are capability parameters enough, or does Oska also need effect annotations on pure-but-nondeterministic operations?
- FFI capability declarations are trusted, not verified. How much can sandboxing at runtime (for example, WASM) enforce them?

## Evaluation

The research does not show that AI-first design choices measurably improve code written by agents. These choices include explicit capabilities, contracts, and strict sink APIs. Oska should test this itself rather than assume it.

- Maintain a benchmark of security-sensitive tasks in the style of CWEval. Each task has functional tests and security tests (func@k vs. func-sec@k).
- Run agents on the same tasks in Oska and in a baseline language. Track functional pass rate, secure pass rate, and how many compiler iterations each task takes.
- Use the results to decide contested design questions, such as mandatory contracts and diagnostic format, instead of relying on intuition.
