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
- **Traits in v0:**
  - Traits can provide default method implementations.
  - There are no associated types.
  - There are no trait objects (`dyn`): all dispatch is static, through generics. A collection that holds different types uses an enum.
  - **Orphan rule:** `impl Trait for Type` is allowed only in the package that defines `Trait` or the package that defines `Type`.
- **Mutation:** a mutating method declares `var self` and can only be called on a `var` binding. No other parameter can be mutated, so everything else flows out through return values.
- **Not included:** inheritance, overloading, higher-kinded types, and user-defined macros.
- **Built-in derivations:** the compiler can generate implementations for a fixed set of traits (equality, hashing, serialization).
- **Modules:** one file is one module, and items are private unless marked `pub`. Imports are explicit `use` paths; glob imports are not allowed.

## Syntax

The syntax is close to Rust's, and departs from it only where Rust's rules depend on context (principle 3). The AST export is described in [`ast.md`](ast.md).

```oska
use std.sql.{Db, query}

pub struct Account { pub id: Int, balance: Int }

impl Account {
    pub fn deposit(var self, amount: Int)
        requires amount > 0
    {
        self.balance = self.balance + amount
    }
}

fn describe(s: Shape) -> String {
    match s {
        Shape.Circle(radius: r) if r > 100 => "big circle"
        Shape.Circle(radius: _) => "circle"
        Shape.Square(side: n) => "square"
    }
}

test "deposit adds to the balance" {
    var a = Account(id: 1, balance: 0)
    a.deposit(amount: 5)
    t.assert_eq(a.balance, 5)
}
```

### Statements

- A newline ends a statement when the line's last token is an identifier, a literal, `self`, `return`, `break`, `continue`, `)`, `]`, `}`, or `?`. This is Go's rule.
- Newlines inside `(...)` and `[...]` never end a statement. Newlines inside `{...}` follow the rule above again, even when the braces are nested inside parentheses, such as a closure body passed as an argument.
- A newline does not end a statement if the next line starts with `requires`, `ensures`, `else`, or `{`. This lets a function header span several lines with contract clauses, and lets `else` start a new line.
- A bare `{ ... }` block cannot be a statement on its own. As a consequence, `let x = { ... }` and blocks after `if`, `for`, and the like are unaffected.
- If an expression continues onto the next line, the current line must end with an operator or `.`.
- There is no `;`.
- Trailing commas are allowed. The formatter decides where they go.
- Blocks are expressions: a block evaluates to its last expression. `return` exits early.
- `let` and `var` always need an initializer (`let x: List<Int> = List.new()`). A declaration without one is a parse error.
- Fields in `struct` and `enum` declarations are separated by commas, even when each field is on its own line.

  Both rules are needed because a line ending in a generic type's closing `>` does not end a statement: `>` is treated as an operator, which continues onto the next line.
- A body-less signature (a trait method, or a prelude function) whose return type ends in `>` still ends at the line break when the next line starts with `fn`, `pub`, `#`, or `}`.

### Names, types, and calls

- **Paths:** `.` is the only path separator. It is used for modules, types, enum variants, fields, and methods (`use std.sql`, `Shape.Circle`, `fs.read_to_string`). Name resolution, not the parser, decides what each segment refers to.
- **Type arguments:** `<T>` appears only in type positions (`List<Int>`, `fn first<T>(xs: List<T>)`). There is no turbofish. Write a type annotation instead, as in `let xs: List<Int> = List.new()`.
- **Bounds:** generic bounds are written inline (`<T: Eq + Hash>`). There are no `where` clauses.
- **Construction** uses call syntax ([ADR 0004](adr/0004-call-style-constructors.md)):
  - Fields declared by name must all be named, in declaration order: `Point(x: 1, y: 2)`.
  - Fields declared by position are passed by position: `Some(x)`, `Meters(3)`.
- **Visibility:** items and fields are private unless marked `pub`. `pub` is not allowed in some places:
  - **`pub use`:** re-exports are not defined in v0, so an import is visible only in its own module.
  - **`pub test`:** tests are run by the test runner and never referenced by name.
  - **`pub impl`:** mark each method `pub` instead.
  - **A trait method:** it takes its visibility from the trait.
- **Argument labels:** any call can label its arguments, as in `copy(src: a, dst: b)`. Labels are optional, and the compiler checks them against the parameter names.
- **Methods** are defined in `impl Type` and `impl Trait for Type` blocks, with `self` as an explicit first parameter. Associated functions are called through the type: `List.new()`.
- **Imports:** `use a.b` or `use a.b.{X, y}`. There is no renaming and there are no glob imports.

### Control flow and patterns

- **Conditionals and matching:** `if cond { } else { }`, and `match x { pattern => expr }` with one arm per line and no commas.
- **Loops:** `for x in xs { }`, `while cond { }`, `break`, and `continue`. There is no `loop` keyword; write `while true` instead.
- **Ranges:** `a..b` is the only range syntax, and it excludes `b`.
- **No `as` casts.** Conversions are explicit calls, such as `I32.from(x)`.
- **Patterns:** `_`, literals, bindings, constructor patterns, and `if` guards. There are no or-patterns, range patterns, or `@` bindings in v0. A string pattern must be a plain literal, with no `{...}` interpolation.
- **Constants:** module-level constants are declared with `const NAME: T = expr`.

### Closures, contracts, and other forms

- **Closures:** `|x| expr` or `|x| { ... }`. A closure literal has no `cap` marker. Whether it is capability-bearing is inferred from what it captures, then checked against the parameter type it is passed to (`fn` or `cap fn`).
- **Contracts:** contract clauses go between the return type and the body, one clause per line. `ensures |r| ...` binds the return value. `implies` binds more loosely than `||` and is right-associative.
- **Attributes:** written `#[name]` or `#[name(args)]`, and allowed only on items. Attribute names come from a fixed set, currently only `#[integration]`, which is allowed only on `test` items. Unknown names are errors.
- **String interpolation:** `{...}` inside a string may hold only a name or a field path, as in `{n}` or `{user.name}`. Compute anything else with `let` first.
- **Sink literals:** `sql"..."` and similar. They cannot contain `{}`.

## Type checking

The checker is `oskac check`, built as its own crate, `oska-check`, on top of the parser. Its diagnostic codes are `E02xx`. Its report format and codes are described in [`checker.md`](checker.md).

### Scope and reporting

- v0 checks a single file plus the **prelude**.
  - `use` resolves only `std.*` paths.
  - Multi-file packages wait for the package manager.
- The checker reports every independent error.
  - Checking continues past an error in one function and moves on to the others.
  - An expression that fails to check gets an error type. The error type suppresses follow-on errors that would only repeat the first one.
- `oskac check --format=json` emits `{"diagnostics": [...], "functions": [...]}`. For each function it reports:
  - `authority`: the built-in capabilities the function can reach. `cap` structs and type arguments are expanded, and each capability keeps its written name, such as `FsRead` or the directory capability `DirRead` that `restrict_to` gives.
  - `via`: the capabilities each parameter supplies.
  - `pure`: whether the function is pure.

  The report is produced even when there are no errors. The human-readable output shows the same information.
- `dbg(...)` is an error when checking with `--release`.

### Inference

- Inference is local and bidirectional. Function signatures are always written out in full; nothing is inferred across function boundaries.
- `let` bindings are inferred from their initializer, and closure parameters from the type the closure is expected to have.
- Generic type arguments are inferred within a single function body.
- An integer literal takes the integer type it is expected to have (`let x: U8 = 200`), and `Int` when there is none. A literal that is out of range for its type is a compile error.
- A generic function is checked once against its bounds, not again for each call. `fn f<T: Eq>(a: T, b: T)` can use `==` but not `+`.

### Method calls

- Methods are looked up on the type's own `impl` first, then in traits implemented for the type. In a single file, every trait is visible.
- If two traits provide a method with the same name, the call is an error. The fix is to call it qualified: `Display.fmt(x)`.

### The prelude

- The prelude is written as Oska declarations and embedded in the compiler. Its function signatures have no bodies, because the runtime implements them. Only prelude functions may omit a body.
- **Built-in capabilities** are opaque structs marked `#[capability]`, an attribute only the prelude can use. Their fields are private, so code outside the prelude cannot construct one: privacy alone stops capabilities from being forged.
- **Automatically imported:** primitive types, `String`, `Bool`, `Option`, `Result`, `List`, `Path`, the built-in capabilities, `Args`, and the core traits (`Eq`, `Hash`, `Display`, `From`).
- **Imported explicitly:** sink and library modules such as `std.sql`, `std.html`, and `std.json` need `use`.
- The prelude grows only as the examples, the design-notes samples, and the tests need it.

### Capability rules

- A type that names a capability in its own fields must be declared `cap`. If it is not, the error comes with a fix that inserts `cap`.
- A generic container is capability-bearing when it is instantiated with a capability, so `List<Stdout>` and `Box<Stdout>` need no `cap` ([ADR 0002](adr/0002-capabilities-are-the-effect-system.md)).
- A method's receiver counts towards its authority. A method on a `cap struct` is therefore never pure.
- **`main` must be a valid root of authority:**
  - Its parameters must be built-in capabilities or program inputs (`Args`).
  - It must return `()` or `Result<(), E>` where `E: Display`.
- A closure that captures a capability-bearing value has type `cap fn`. Passing it where a plain `fn` is expected is an error.

### Patterns and values

- **Capitalization is enforced** ([ADR 0005](adr/0005-capitalization-is-enforced.md)):
  - Types, variants, and traits start with an uppercase letter.
  - Variables, functions, fields, and parameters start with a lowercase letter.
  - A bare uppercase name in a pattern is always a fieldless variant.
- **`match` must be exhaustive:**
  - The check covers nested constructor patterns. Guards never count towards it.
  - Matching on `Int` or `String` needs a `_` or binding arm.
  - The error lists the missing patterns, and its fix adds arms for them.
  - Any arm after a binding or `_` arm is unreachable, which is an error.
- **Ignoring a `Result` is an error.** To discard one on purpose, write `let _ = f()`.
- **`?` works on `Result` and on `Option`,** inside a function that returns the same kind. To cross from one to the other, convert explicitly, for example with `.ok_or(err)?`.
- **Shadowing is allowed.** For example, `let email = Email.parse(email)?` replaces the raw value with the validated one.
- **`+` is defined only for numbers.** Build strings with interpolation.
- **Recursive types need no indirection** (`struct Node { next: Option<Node> }`). A type with no base case, such as `struct Loop { me: Loop }`, is an error.

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

Trust is a flag that the checker tracks on text types ([ADR 0006](adr/0006-trust-is-a-flag-on-text.md)). `Untrusted<String>` is how a signature spells "this text may be untrusted". It is not a wrapper type.

- **Sources.** Text read through a capability (files, network, stdin, and the environment through `Env.get`) is untrusted. So is text from program inputs (`Args.get`).
- **Flow.**
  - Trusted text may flow into any slot that accepts untrusted text. Untrusted text may never flow into a trusted slot.
  - The checker enforces this one-way rule with the same mechanism that stops a `cap fn` from flowing into a `fn`.
- **Propagation.**
  - Prelude text operations follow their inputs. If any text input is untrusted, every text type in the result is untrusted. For example, `.lines()` returns `List<Untrusted<String>>`.
  - Interpolating an untrusted value makes the whole string untrusted. Every interpolation hole must be `Display`.
  - Inside a generic body, text that comes from a type parameter is untrusted, because the parameter may stand for untrusted text. That covers interpolating a `T` and the text result of a call on a `T` receiver or with a `T` argument, so `fn f<T: Display>(x: T) -> String { x.to_string() }` is an error.
  - A generic function or trait method follows the trust of its type arguments. If what any generic, or `Self` in a qualified trait call, is instantiated with holds untrusted text, every text type in the result is untrusted. So `fn show<T: Display>(x: T) -> String` returns untrusted text for an untrusted `x`, whether its body uses `x.to_string()` or `"{x}"`.
  - Results that are not text are trusted. For example, `.count()` returns a trusted `Int`.
- **User code.**
  - In user code, a `String` parameter, field, or return type means **trusted**.
  - Code that accepts raw input declares `Untrusted<String>`.
  - User functions are explicit about trust. They cannot be generic over it.
  - An impl method may return trusted text where its trait declares `Untrusted<String>`, because trusted text may flow into untrusted.
  - The `message` of `IoError`, `SqlError`, and `ProcError` is `Untrusted<String>`, because it can carry text from outside the program.
- **Validation.**
  - Checks that produce non-text results need nothing special. That includes `len`, `contains`, `starts_with`, `parse_int`, and matching against string literals.
  - A **validator** turns untrusted text into trusted text with `.assume_trusted()`. That call is a **trust assertion**.

```oska
pub fn parse(s: Untrusted<String>) -> Result<Email, ParseError> {
    if !s.contains("@") { return Err(ParseError.Invalid) }
    Ok(Email(raw: s.assume_trusted()))
}
```

- **Sinks.** A **data position** accepts untrusted values. A **structural position** rejects them. A structural position is any parameter declared `String`.

| Sink | Data position (untrusted OK) | Structural position (trusted only) | Not provided |
| --- | --- | --- | --- |
| SQL (`std.sql`) | Parameters: `db.query(sql"SELECT * FROM users WHERE id = ?", [id])` | Query text, which must be an `sql"..."` literal | Concatenation or interpolation |
| Process (`std.proc`) | Arguments: `run("git", ["log", branch])` | The program name | A shell string. Running a shell is a separate, explicitly named capability. |
| HTML (`std.html`) | Text, which is escaped for its context | `TrustedHtml` | Writing untyped strings into markup |
| Paths | `Path.from` (see below) | The directory passed to `restrict_to` | `..` traversal outside a restricted capability |
| Network | Request bodies | The host passed to `NetConnect` | |
| Output | `Stdout.println` and `Stderr` | | |

- **Sink modules.**
  - The sink modules are part of the prelude, and each needs a `use` (for example, `use std.sql`). A file's imports therefore show which sinks it talks to.
  - A sink literal's tag must name an imported sink, so `sql"..."` requires `use std.sql`. An unknown tag is an error with a "did you mean" fix.
  - Sink literals cannot contain `{}` interpolation.
  - Building a query string by hand is a compile error with a stable diagnostic code. It is not a lint.
- **Paths.**
  - `Path.from` accepts untrusted text, because the protection for paths comes from **attenuation**, not from trust tracking.
  - Attenuate a filesystem capability with `restrict_to` before reading paths a user chose. The runtime rejects any path that escapes the restricted directory.
  - A restricted and an unrestricted `FsRead` are the same type, so the checker cannot tell them apart.
- **Secrets.**
  - `env.secret("API_KEY")` returns `Secret<String>`. In contrast, `env.get` returns `Untrusted<String>`.
  - A `Secret` has no `Display`; `dbg` prints it as `<redacted>`.
  - A `Secret` cannot be interpolated, nor can any value that holds one, such as a `List<Secret<String>>` or a struct with a `Secret` field.
  - A `Secret` cannot be compared with `==`, because that would leak timing information. Use `a.constant_time_eq(text)` instead, or `a.constant_time_eq_secret(b)` to compare two secrets without exposing either. A fix rewrites `==` to the right one.
  - The only way to read a secret's value is `.expose()`, and the exposed value is trusted.
- **Audit.**
  - Every trust assertion (`.assume_trusted()`) and every secret exposure (`.expose()`) is an **audit site**.
  - `oskac check --format=json` lists all audit sites, with their spans, in an `"audit"` array. The human-readable report lists them too.
- **Scope.**
  - For now, trust is tracked on text only. `Bytes` will be added when the first binary API needs it.
  - The diagnostic codes are `E03xx`. The range covers trust, sinks, and secrets, and also the `std` modules: imports, qualified names, and sink literal tags.
- **Runtime implementations.** The sink modules start as signature-only prelude declarations. Their runtime implementations arrive with the interpreter.

## Concurrency

- Lightweight tasks run inside `scope |s| { ... }` blocks.
  - `s.spawn(|| work())` starts a task and returns a handle, and `h.join()` returns the task's result.
  - A scope does not exit until all of its tasks have finished.
  - The scope handle cannot escape the block.
  - Nothing can spawn a task outside a scope.
- There is no `async`/`await`, so functions do not come in async and non-async versions.
- Tasks can share immutable values freely, and they receive capabilities as arguments like any function.
- **Deterministic parallelism** needs no capability, so pure code can use it. `par_map` and `join` are library functions built on scopes.
- **Channels and `select`** are nondeterministic, so they require `Sched`.

## Contracts

```oska
fn withdraw(acct: Account, amount: Int) -> Result<Account, Error>
    requires amount > 0
    ensures |r| r.is_ok() implies r.unwrap().balance >= 0
{
    if amount > acct.balance {
        return Err(Error.InsufficientFunds)
    }
    Ok(Account(id: acct.id, balance: acct.balance - amount))
}
```

- Contracts are machine-checkable specifications. Code an agent writes only has to be *checkable*, not right the first time.
- A contract may only call pure functions. The compiler enforces this.
- In v0, contracts are checked at runtime in all builds.
- Later, the compiler will prove decidable contracts statically with an SMT solver and fall back to runtime checks for the rest. It will report which contracts were proved and which are runtime-checked.

## Testing

- Tests are top-level items: `test "a descriptive name" { ... }`. They take no parameters. The name is a plain string, with no `{...}` interpolation, and a test is never `pub`.
- Inside a test, the built-in value `t` holds deterministic stand-in capabilities: an in-memory `t.fs`, a fixed `t.clock`, a seeded `t.random`, and a recording `t.stdout`.
- Tests that need real capabilities opt in with `#[integration]`. They run only with `oskac test --integration`.
- `oskac test file.osk` runs every `test` block and reports each one as passed or failed. With `--format=json`, the report is JSON.
- **The stand-ins:**
  - an in-memory `Fs`
  - a `Clock` fixed at the Unix epoch
  - a `Random` with a fixed seed
  - a recording `Stdout`; `t.printed()` returns one entry per `println`

## Runtime

The interpreter is `oskac run file.osk -- args...`, built as the crate `oska-run`.

### Pipeline

- `oskac run` type-checks the program first. If the check reports any diagnostic, the program does not run, and the diagnostics are printed the same way `oskac check` prints them.
- The checker produces a **checked program**, and the interpreter executes it ([ADR 0007](adr/0007-interpreter-runs-the-checked-program.md)). The interpreter never re-resolves names or dispatch.

### Values

- Values are reference-counted. Mutating a `var` copies the value first if it is shared ([ADR 0001](adr/0001-value-semantics-with-reference-counting.md)).
- Every value carries a runtime type tag. A trait call inside generic code uses the tag to find the right impl.
- **Trust exists only at compile time.** At runtime, `Untrusted<String>` is an ordinary string.
- **`Secret` stays a real wrapper at runtime,** so `dbg` prints it as `<redacted>`.

### Capabilities

- The runtime grants exactly the capabilities that `main` declares.
- `Args.get(0)` is the path of the program being run.
- **Implemented in v0:**
  - `Stdin`, `Stdout`, `Stderr`
  - `Fs`, `FsRead`, `FsWrite`
  - `Args`, `Env`, `Env.secret`
  - `Clock`
  - `Random`, with a built-in pseudo-random generator
  - `std.html`: `text` escapes `&`, `<`, `>`, `"`, and `'`; `TrustedHtml` markup passes through unescaped
- **Not yet implemented:** `Net`, `std.proc`, and `std.sql`. `oskac run` refuses, before execution starts and with a stable code, any program that uses them.
- **Filesystem confinement.** A restricted `Fs` resolves every `Path` against its root. Before checking, it normalizes `..` and follows symlinks. A path that escapes the root is rejected, and so is an absolute path or an empty one.
- **Prelude functions without a body** are each backed by a built-in implementation in the runtime. A test enforces that none is missing.

### Failures

- **Panics** have stable codes, a message, and the span of source code that panicked. With `--format=json`, they print in the same shape as diagnostics. The process exits with code **101**.

| Code | Panic |
| --- | --- |
| `P0001` | integer overflow |
| `P0002` | index out of bounds |
| `P0003` | `todo()` reached |
| `P0004` | division by zero |

- **When `main` returns `Err(e)`,** the runtime prints `e.to_string()` to stderr and exits with code **1**.
- Exit code 1 means the program failed in a way it expected. Exit code 101 means it has a bug.

### Strings

- Strings are UTF-8.
- `len()` counts Unicode code points. `byte_len()` counts bytes.
- Indexing or slicing that would split a character panics.

### Concurrency in v0

- Spawned tasks run one at a time, in the order they were spawned, and `join` returns the task's result. This preserves the semantics, because tasks share only immutable values.
- Calling `spawn` on a scope that has already finished panics.
- Parallel execution and `Sched` channels come later.

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
- **Agent-facing output:** structured diagnostics, plus a stable, documented AST/JSON export (see [`ast.md`](ast.md)).
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
- WASM component imports: what is the exact mapping between Oska capabilities and WASI interfaces?
