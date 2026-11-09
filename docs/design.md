# Oska design notes

Working draft. Everything here is open to change.

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
- Untrusted input is tainted (`Untrusted<T>`) until it is validated, so it cannot reach sinks such as SQL, shell, or HTML without a validation step.

## Capabilities

```oska
fn read_config(fs: FsRead, path: Path) -> Result<Config, ConfigError> {
    let text = fs.read_to_string(path)?
    Config.parse(text)
}
```

- The program entry point (`main`) declares the root capabilities it needs; the runtime grants exactly those.
- Capabilities can be *attenuated*: `fs.restrict_to("/etc/myapp")` returns a narrower capability.
- Dependencies can only use capabilities you pass them, which limits supply-chain blast radius.

## AI-first features (to explore)

- Contracts (`requires` / `ensures`) checked at runtime in debug builds and used as specs by tooling.
- Structured diagnostics and a stable, documented AST/JSON export for agents.
- `intent` doc blocks the compiler can cross-check against signatures (for example, flagging "pure" functions that take capabilities).
- Deterministic formatting with a single canonical style.

## Open questions

- Memory management: ownership/borrowing vs. region-based vs. GC with value types.
- Compilation target: LLVM, Cranelift, or WASM first.
- Concurrency model and how capabilities flow across tasks.
- `unsafe` / FFI boundary design.
