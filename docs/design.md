# Oska design notes

This is a working draft. Decisions that are hard to reverse are recorded in [`adr/`](adr/), and terms are defined in [`GLOSSARY.md`](../GLOSSARY.md). The background research is in [`research/2026-11-ai-first-secure-language.md`](research/2026-11-ai-first-secure-language.md).

## Principles

1. **Secure by default, unsafe only on explicit request.** The safe path is the shortest path. Anything dangerous has to be opted into, is named, and can be found with grep.
2. **No ambient authority.** I/O, network, filesystem, environment, clock, randomness, scheduling, and process spawning are *capabilities*: values passed as arguments. A function without a capability-bearing parameter cannot perform that effect.
3. **Legible to humans and models.** A function signature tells you everything the function can do. There is one obvious way to write things, and no syntax depends on context. Everything that runs is visible on the page: there are no user macros ([ADR 0003](adr/0003-no-user-defined-macros.md)), no glob imports, and no code that runs when a module loads.
4. **Machine-first tooling.** Every diagnostic has a stable code and a precise span. Where possible it also carries a fix that tools can apply automatically. Diagnostics are emitted as JSON with `--format=json`.

## Safety baseline

- **Memory safety** comes from value semantics and reference counting. There are no raw pointers, lifetimes, or shared mutable references ([ADR 0001](adr/0001-value-semantics-with-reference-counting.md)).
- **No null:** absent values are `Option<T>`.
- **Errors as values:** functions return `Result<T, E>`. Panics are not catchable.
- **Checked integers:** `Int` is a 64-bit signed integer, and overflow panics. Sized types (`I32`, `U8`, …) convert only explicitly, wrapping and saturating arithmetic are explicit operations, and arbitrary-precision numbers use `BigInt`.
- **No implicit conversions,** with one exception: `?` converts an error type through an explicitly written `impl From<E1> for E2`.
- **Immutable by default:** bindings are immutable (`let`) unless declared `var`.
- **No global mutable state.** At module level you can only declare constants, functions, and types.
- **Untrusted data:** text and bytes from outside the program arrive as **untrusted values** (`Untrusted<String>`, `Untrusted<Bytes>`). See [Untrusted data and sinks](#untrusted-data-and-sinks).

## Types and modules

- **Types:** structs, sum types (enums with data) with exhaustive `match`, generics, and traits (explicitly implemented interfaces).
- **Not included:** inheritance, overloading, higher-kinded types, and user-defined macros.
- **Built-in derivations:** the compiler can generate implementations for a fixed set of traits (equality, hashing, serialization).
- **Modules:** one file is one module, and items are private unless marked `pub`. Imports are explicit `use` paths; glob imports are not allowed.

## Capabilities

```oska
fn read_config(fs: FsRead, path: Path) -> Result<Config, ConfigError> {
    let text = fs.read_to_string(path)?   // Untrusted<String>
    Config.parse(text)                     // validation: Untrusted<String> -> Config
}
```

Capabilities are the effect system. Oska has no separate effect annotations ([ADR 0002](adr/0002-capabilities-are-the-effect-system.md)).

### Root of authority

```oska
fn main(fs: Fs, net: Net, args: Args) -> Result<(), AppError> {
    let config_fs = fs.restrict_to("/etc/myapp").read_only()
    let config = read_config(config_fs, Path.new("config.toml"))?
    serve(net.connect_only(), config)
}
```

- `main` is the only **root of authority**. The runtime grants exactly the capabilities its signature declares, and grants them to nothing else. You can read a whole program's authority from that one signature.
- `main` can also receive **program inputs** such as `Args`. These are plain data and grant no authority.

**Built-in capabilities:**

| Capability | Grants |
| --- | --- |
| `Stdin`, `Stdout`, `Stderr` | Standard streams |
| `Fs` | Filesystem. Attenuates to `FsRead`, `FsWrite`, or a single directory. |
| `Net` | Network. Attenuates to `NetConnect` or `NetListen`. |
| `Env` | Environment variables. These can hold secrets. |
| `Clock`, `Random` | Nondeterministic inputs |
| `Sched` | Channels and `select` |
| `Spawn` | Starting OS processes |

### Rules

- Capabilities cannot be forged. You can only get one by **attenuation** from a capability you already hold, or by being passed one.
- Attenuation never widens authority. There is no way to get a broader capability back from a narrower one.
- If code has no value of the right capability type, it cannot perform the effect, and the compiler rejects it.

### Capability-bearing types

- Any type that contains a capability must be declared with `cap`, for example `cap struct Logger { out: Stdout }`.
- Omitting `cap` is a compile error that comes with an automatic fix.
- A function type `fn(T) -> U` accepts only pure closures. Accepting a closure that captures a capability requires `cap fn(T) -> U`.
- A **pure function** has no capability-bearing parameters. It cannot perform effects, and it is deterministic.

There are two deliberate exceptions to the rules above:

- `dbg(expr)` writes to stderr in debug and test builds. In release builds it is a compile error.
- Panics stop the current task but are not effects on the outside world.

### Package authority

- A dependency can only use the capabilities you pass it.
- The lockfile records each dependency's **authority**: the union of the capability-bearing types in its public API.
- If an upgrade widens a dependency's authority, the build fails until someone approves the change.

## Untrusted data and sinks

- **Sources.** Text and bytes read through a capability (files, network, stdin, environment) or from program inputs are untrusted.
- **Propagation.** Text operations keep a value untrusted. `.lines()` produces untrusted lines, and interpolating an untrusted value makes the whole string untrusted.
- **Validation.** Converting to a non-text or validated type makes a value trusted. Examples: `.count() -> Int`, `Email.parse(s) -> Result<Email, _>`.
- **Sinks.** A **data position** accepts untrusted values. A **structural position** rejects them.

| Sink | Data position (untrusted OK) | Structural position (trusted only) | Not provided |
| --- | --- | --- | --- |
| SQL | Parameters: `db.query(sql"SELECT * FROM users WHERE id = ?", id)` | Query text, which must be an `sql"..."` literal | Concatenation or interpolation |
| Process | Arguments: `proc.run("git", ["log", branch])` | The program name | A shell string. Running a shell is a separate, explicitly named capability. |
| HTML | Template text, escaped for its context | `TrustedHtml` | Writing untyped strings into markup |
| Paths | File names joined under a restricted `Fs` | The directory passed to `restrict_to` | `..` traversal outside a restricted capability |
| Network | Request bodies | The host passed to `NetConnect` | |
| Logging | Structured field values | | Interpolating raw strings into log lines |

- Sink literals (`sql"..."` and similar) cannot contain `{}` interpolation.
- Building a query string by hand is a compile error with a stable diagnostic code, not a lint.
- **Secrets:**
  - `env.secret("API_KEY")` returns `Secret<String>`.
  - A `Secret` displays and debug-prints as `<redacted>` and cannot be interpolated.
  - The only way to read the value is `.expose()`, which `oskac audit` lists.
  - `env.get` returns `Untrusted<String>`.
- The standard library ships these sinks from the start: `sql`, `html`, `proc`, an HTTP client and server, `json`, and structured `log`. The safe path should be the one that is already there.

## Concurrency

- Lightweight tasks run inside `scope { ... }` blocks. A scope does not exit until all of its tasks have finished.
- There is no `async`/`await`, so functions do not come in async and non-async versions.
- Tasks can share immutable values freely, and they receive capabilities as arguments like any function.
- **Deterministic parallelism** (`par_map`, `join`) needs no capability, so pure code can use it.
- **Channels and `select`** are nondeterministic, so they require `Sched`.

## Contracts

```oska
fn withdraw(acct: Account, amount: Int) -> Result<Account, Error>
    requires amount > 0
    ensures |r| r.is_ok() implies r.unwrap().balance >= 0
{ ... }
```

- Contracts are machine-checkable specifications. Code an agent writes only has to be *checkable*, not right the first time.
- A contract may only call pure functions. The compiler enforces this.
- In v0, contracts are checked at runtime in all builds.
- Later, the compiler will prove decidable contracts statically with an SMT solver and fall back to runtime checks for the rest. It will report which contracts were proved and which are runtime-checked.

## Testing

- `test` blocks get deterministic stand-in capabilities by default: an in-memory `Fs`, a fixed `Clock`, a seeded `Random`, and a recording `Stdout`.
- Tests that need real capabilities opt in with `#[integration]`.

## Unsafe and FFI

Escape hatches are where memory-safety and capability guarantees break, so they have to be rare, visible, and contained.

- **v0 has no FFI and no `unsafe`.** The runtime implements all built-in capabilities.
- **Later: WASM component imports.** These will be the first form of FFI. Their Oska signatures will be typed with the capabilities they need, and the WASM sandbox enforces those capabilities at runtime as well.
- **Later: native C FFI.** It will wait for a native backend.

Once escape hatches exist, the following policy applies:

- `unsafe { ... }` blocks and `extern` declarations are the only places where the safety baseline can be suspended.
- Each package declares in its manifest whether it uses `unsafe` or FFI. A package that uses either without declaring it is rejected.
- The application manifest lists which dependencies are allowed to use `unsafe` or FFI. This is modeled on Pony's `--safe`.
- `oskac audit` lists every `unsafe` site, FFI site, and `.expose()` call in the dependency graph as JSON.

## Toolchain

- **Execution:** a tree-walking interpreter comes first, then WASM/WASI as the first compiled target.
- **Agent-facing output:** structured diagnostics, plus a stable, documented AST/JSON export.
- **Formatting:** deterministic, with a single canonical style.

## Evaluation

The research does not show that AI-first design choices measurably improve the code agents write. Oska tests this itself instead of assuming it.

- **Benchmark:** a set of security-sensitive tasks in the style of CWEval. Each task has functional tests and security tests (func@k vs. func-sec@k).
- **Baselines:** the same tasks are also run in Rust and in Python:
  - Rust isolates what capabilities and sinks add on top of memory safety.
  - Python is what models write by default.
- **Metrics:** functional pass rate, secure pass rate, and how many compiler iterations each task takes.
- **Start:** the benchmark starts as soon as the interpreter can run the first task.
- **Gate:** contested decisions are not finalized without benchmark data. Examples are whether contracts should be mandatory, the SMT budget, and the diagnostic format.

## Open questions

- Should contracts be mandatory, for example on `pub` functions or on functions that take untrusted values? How much SMT power is worth the extra compile time and the extra difficulty for models? The evaluation should decide this.
- Generics and traits: what are the rules for trait coherence and orphan implementations?
- WASM component imports: what is the exact mapping between Oska capabilities and WASI interfaces?
