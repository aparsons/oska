# Oska checker report

`oskac check <file.osk> [--format=json] [--release]` type-checks one file against the embedded prelude. The rules it enforces are in the "Type checking" section of [`design.md`](design.md). This page describes what it prints. The codes `oskac run` reports, its refusals and panics (`P`-codes), are in [`runtime.md`](runtime.md).

The exit code is non-zero when there are diagnostics. Without `--format=json`, diagnostics go to stderr and the authority report to stdout, one function per line, such as `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`. Each function's attenuations follow it, indented, such as `  FsRead -> DirRead: restrict_to "docs" (bytes 88..112)`. One line per audit site comes last, such as `audit: assume_trusted (bytes 52..73)`.

An **attenuation** is a call of one of the prelude's attenuation methods (`restrict_to`, `restrict_to_file`, `read_only`, `write_only`) on a built-in capability. The report lists each one under the function it is in (a closure's belong to the function around it), with the capability kind it narrows from and to, and its root:

- `restrict_to "docs"` has a **literal root**: a string literal, a module constant holding one, or `Path.new` of either. Narrowing a directory capability gives the path relative to its own root, as written.
- `restrict_to computed at runtime` has any other root: a variable, an interpolated string, `Path.from(untrusted)`. In JSON its `root` is `null`.
- `read_only` and `write_only` take no root, so they show none.

A reviewer reads a program's filesystem reach straight from these lines: `FsRead -> DirRead: restrict_to "docs"` and `FsWrite -> DirWrite: restrict_to "_site"` mean the program reads `docs` and writes `_site`. Whole-filesystem access shows as `restrict_to "/"`.

Type and trait names in signatures and declarations are resolved, declarations are checked (`cap`, `main`, capitalization, recursive types, and where a `Scope` may appear), functions, methods, and impls are checked for duplicates, and under `--release` every body is scanned for `dbg(...)` and `todo()`. A `use` must name a `std.*` module that exists (`E0294`, `E0295`); see [`std` modules](#std-modules) for which do. Function, method, constant, and test bodies are type-checked with local, bidirectional inference: `let` types come from initializers, the expected type flows into literals, generic constructors, and closures, and an expression that fails gets an error type so it causes only one diagnostic.

Text carries a trust flag (ADR 0006). `Untrusted<String>` is untrusted text; plain `String` is trusted. Text from outside the program is untrusted: `DirRead.read_to_string`, `FileRead.read_to_string`, `Stdin.read_line`, `Env.get`, and `Args.get` return it. Trusted text may be used wherever untrusted text is accepted, so a data position such as `Stdout.println` or `Path.from` takes both; untrusted text is never accepted where trusted `String` is required (`E0300`). A validator vouches for text it has checked with `.assume_trusted()`, which returns trusted `String`; each call is a trust assertion, listed in the report's `audit` array. `Untrusted<...>` marks text only (`E0320`).

`Env.secret` returns a `Secret<String>`, which code cannot construct itself (`E0271`). A secret cannot be interpolated, nor can a value that holds one (`E0350`). It is not `Eq`, so `==` and `!=` on one are errors (`E0351`); compare it with `.constant_time_eq(text)`, which takes trusted or untrusted text, or with another secret through `.constant_time_eq_secret(other)`, which exposes neither. It is not `Display`, so it cannot be printed (`E0352`). The only way to read its value is `.expose()`, which returns trusted text; each call is a secret exposure, listed in the report's `audit` array.

Untrusted-ness carries through every route. Prelude text operations are trust-polymorphic: the prelude marks each with `#[follows_trust]`, an attribute only the prelude may write. Each call gets a fresh trust variable that its receiver and text arguments join into, and every text type in its result has that trust, so `untrusted.trim()` and `untrusted.lines()` are untrusted while `untrusted.len()` and `untrusted.contains("x")` are trusted `Int` and `Bool`. Any other function's receiver is checked like an argument. An interpolated string is untrusted when text in any hole is, and every hole must be `Display` (`E0265`). Inside a generic body, text that comes from a type parameter is untrusted: interpolating a `T`, and the text result of a call on a `T` receiver or with a `T` argument. A generic function or trait method also follows the trust of its type arguments at each call: when the result holds a fixed `String` and what any generic (or `Self`, in a qualified trait call) is instantiated with holds untrusted text, that text in the result is untrusted, so `fn show<T: Display>(x: T) -> String` cannot launder an untrusted `x`. An impl method may return trusted text where its trait declares `Untrusted<String>`. The `message` of `IoError`, `SqlError`, and `ProcError` is untrusted, and so is their text: a struct or enum **holds untrusted text** when a field is untrusted text or a type that holds it, at any depth, and such a value turns into untrusted text wherever it becomes text. That covers interpolating it, a generic's type argument, `Self` in a qualified trait call such as `Display.to_string(e)`, and a method of a prelude or `std` type, such as `e.to_string()` or `IoError.to_string(e)`, whose body the checker cannot see. A method the module declares is checked against its own signature, so its `to_string(self) -> String` cannot return `self.cause.to_string()` for an `IoError` field without `.assume_trusted()`. `==` and `!=` compare text of either trust, and a trait impl for `String` covers both. Where values meet, trusted and untrusted text join to untrusted rather than conflicting: `if` and `match` branches, the arguments of a generic function for one type parameter, and the elements of a list. Inference variables are solved to trust variables, and the checker keeps every flow between them, so text that has already been used where trusted `String` is required rejects untrusted text that arrives later, as with `names.push(arg)` after `greet(names.get(0)?)`.

Contract clauses are type-checked but not evaluated: `requires` and `ensures` conditions must be `Bool`, they see the parameters (and `self`), `ensures |r|` binds `r` to the return type, and a contract may call only pure functions (`E0280`). A trait method's contract is checked even without a body.

A `test` body sees one built-in value, `t`, of the prelude type `TestContext`. Its fields are stand-in capabilities: `t.fs` (an in-memory `Fs`), `t.clock` (a fixed `Clock`), `t.random` (a seeded `Random`), and `t.stdout` (a recording `Stdout`). Its methods are the assertions `t.assert(cond)`, `t.assert_eq(actual, expected)`, `t.assert_ne(actual, other)`, and `t.fail(message)`, and `t.printed()` returns the lines printed to `t.stdout`. An `#[integration]` test gets the same `t`; the runtime decides which capabilities are real. A test produces no value, so its last expression is a statement like the others.

## Report

```json
{
  "diagnostics": [
    {"code":"E0200","message":"unknown type `Strng`; did you mean `String`?","span":{"start":8,"end":13},"fix":{"message":"replace with `String`","edits":[{"span":{"start":8,"end":13},"replacement":"String"}]}}
  ],
  "functions": [
    {"name":"count_lines","authority":["FsRead"],"via":[{"param":"fs","capabilities":["FsRead"]}],"pure":false},
    {"name":"Logger.log","authority":["Stdout"],"via":[{"param":"self","capabilities":["Stdout"]}],"pure":false},
    {"name":"add","authority":[],"via":[],"pure":true}
  ],
  "audit": [
    {"kind":"assume_trusted","span":{"start":412,"end":430}}
  ],
  "attenuations": [
    {"function":"count_lines","method":"restrict_to","span":{"start":150,"end":176},"source":"FsRead","result":"DirRead","root":"docs"}
  ]
}
```

- **`diagnostics`**, in source order. Each uses the same object as parser diagnostics (see [`ast.md`](ast.md)): `code`, `message`, `span` (byte offsets), and `fix` when the repair is unambiguous. A parse error is reported alone, with empty `functions`, `audit` and `attenuations`.
- **`functions`**: one entry per function and `impl` method, in source order. The report is produced even when there are diagnostics.
  - `name`: the function's name, or `Type.method` for a method.
  - `authority`: the built-in capabilities the function can reach, in order of first appearance, without repeats. `cap` types are expanded through their fields, and type arguments are expanded, so `List<Stdout>` reaches `Stdout`. Each capability is reported by its own name: an attenuation to a directory shows as the directory capability it gives, such as `DirRead`.
  - `via`: each capability-bearing parameter and the capabilities it supplies, in parameter order. A method's receiver appears as `self`. A `cap fn` parameter is listed with no capabilities, because a closure's capabilities are not written in its type; it still makes the function impure.
  - `pure`: `true` when no parameter, the receiver included, is capability-bearing.
- **`audit`**: every **audit site**, in source order. It is always present, and is `[]` when there are none.
  - `kind`: what the call does. `assume_trusted` is a **trust assertion**: `.assume_trusted()` turns untrusted text into trusted text, as a validator does. `expose` is a **secret exposure**: `.expose()` reads a `Secret`'s value.
  - `span`: the whole call expression, such as `s.assume_trusted()`.
- **`attenuations`**: every call that narrows a capability, in source order. It is always present, and is `[]` when there are none. The entries come from the prelude's attenuation methods, so a new capability kind with such a method appears with no change to the checker.
  - `function`: the function the call is in, named as in `functions`.
  - `method`: `restrict_to`, `restrict_to_file`, `read_only` or `write_only`.
  - `source` and `result`: the capability kinds before and after, such as `FsRead` and `DirRead`.
  - `root`: the root as a string when it is a literal or a module constant, and `null` when it is computed at runtime or the method takes none (`read_only`, `write_only`). The human report tells the two apart.
  - `span`: the whole call expression, such as `read.restrict_to("docs")`.

## The prelude

The prelude is Oska source, `crates/oska-check/src/prelude.osk`, parsed with `parse_prelude`. It declares the built-in capabilities as `#[capability]` structs with private fields, the core types (`Option`, `Result`, `List`, `Range`, `Path`, `Args`, `IoError`) with the few methods the examples and tests use, structured concurrency (`Scope`, whose `spawn(task: cap fn() -> T)` returns a `Task<T>`, whose `join()` returns `T`; `scope |s| { ... }` binds `s: Scope`), the `dbg` and `todo` functions, `TestContext` (the type of `t` in tests), and the core traits (`Eq`, `Hash`, `Display`, `From`). The primitive types (`Int`, `I8` to `I64`, `U8` to `U64`, `Bool`, `String`) and `()` are built into the checker. The prelude implements `Display` for `String`, `Int`, and `IoError`. User code cannot redeclare any of these names.

### `std` modules

A `std.*` module is also prelude-mode Oska source, `crates/oska-check/src/std/<name>.osk`, registered in `STD_MODULES` in `src/prelude.rs`. Its declarations are in scope only in a module that imports it. `use std.sql` imports the module: code reaches its exports qualified, as types and as values (`sql.Db`, `sql.connect(...)`, `sql.Db.query`), and writing one unqualified is `E0340`. `use std.sql.{Db, connect}` imports those names unqualified and does not bind `sql`. There are no glob imports. A brace group or a qualified path may only name what the module exports (`E0312`). Without the `use`, its names are unknown (`E0200`). A local variable shadows a module name. A name the module declares but does not export is private to it (`E0360`). The user module may declare a name that an imported module also declares, as long as it is not imported unqualified: after `use std.sql`, a user `struct Row` is `Row`, and the module's is `sql.Row`. The modules that exist are:

| Module | Exports | Sink literal |
| --- | --- | --- |
| `std.sql` | `SqlLiteral`, `Db` (`query`), `connect`, `Row` (`get`), `SqlError` | `sql"..."` has type `SqlLiteral` |
| `std.proc` | `run`, `Output` (`status`, `stdout`), `ProcError` | None |
| `std.html` | `Html` (`raw`, `concat`, `render`), `text`, `TrustedHtml` (`new`) | None |

A sink literal's tag names the module that provides it. The literal has the type that module declares for it, and the module must be imported (`E0310`). A tag no module provides is `E0311`. The sink modules are signature-only. `std.html` has a runtime: `text` escapes `&`, `<`, `>`, `"`, and `'`, so its output is safe in an element's content and in a quoted attribute value, and `TrustedHtml` markup passes through unescaped. `std.sql` and `std.proc` have none yet.

Each sink separates a **data position**, declared `Untrusted<String>`, from a **structural position**, declared `String` or a sink literal type. Untrusted text in a `String` structural position is `E0300`; any text where a sink literal is required is `E0341`.

| Sink | Data position | Structural position |
| --- | --- | --- |
| `std.sql` | `Db.query`'s `params: List<Untrusted<String>>` | `Db.query`'s `query: SqlLiteral`; `connect`'s `url` |
| `std.proc` | `run`'s `args: List<Untrusted<String>>` | `run`'s `program`; `run` also needs `Spawn` |
| `std.html` | `text`'s argument, which is escaped | `TrustedHtml.new`'s `markup` |
| Paths | `Path.from`, also as the `dir` of a directory capability's `restrict_to` | `restrict_to`'s `dir` and `restrict_to_file`'s `file` on `Fs`, `FsRead`, and `FsWrite` |
| Network | `Connection.send`'s `body` | `NetConnect.connect`'s `host` |

Values that come back from a sink are untrusted: `Row.get`, `Output.stdout`, `Html.render`, and `Connection.receive`. `Db` and `Connection` are capability-bearing (`Db` holds a `NetConnect`), and every sink type except the error types has private fields, so code outside the module can't construct one (`E0271`).

## Syntax diagnostic codes

The lexer and the parser report these. A syntax error is the only diagnostic of a file: nothing past it is parsed, so `oskac check` reports no checker code beside it. A file that parses reaches the checker, and its codes are in the [checker table below](#diagnostic-codes). Each code below has a failing example and its fix, as the checker codes do (see [Examples](#syntax-examples)).

| Code | Meaning | Fix |
| --- | --- | --- |
| `E0001` | A character that starts no token, such as `$`. Spans the character. | None. |
| `E0002` | A string literal that is not closed before the end of the file. Spans the literal from its opening quote. | None. |
| `E0003` | An invalid escape in a string: a backslash followed by anything but one of the escapes. Spans the backslash. | None. |
| `E0004` | A `{` or `}` in a sink literal such as `sql"..."`, which never interpolates. Write `\{` or `\}` for a literal brace. | None. |
| `E0005` | A `;`: Oska has no semicolons, because a line break ends a statement. | Remove the `;` when it ends a line. |
| `E0100` | The parser expected an expression, a type, or a pattern and found something else. | None. |
| `E0101` | The parser expected a specific token, such as a `)`, and found another. The message names it. Items must also be separated by a line break, so two items on one line give this code. | None. |
| `E0102` | A `{...}` hole in a string that holds more than a name or a field path, such as `{n + 1}`. Compute the value with `let` first. | None. |
| `E0103` | A bare `{ ... }` block used as a statement. | None. |
| `E0104` | A `let` or `var` without an initializer. | None. |
| `E0105` | A chained comparison such as `a < b < c`. Comparison operators do not chain. | None. |
| `E0106` | An assignment whose left side is not a name or a field path, such as `x + 1 = 2`. | None. |
| `E0107` | A statement at module level, where only items are allowed. | None. |
| `E0110` | An `ensures` clause without a `\|r\|` binding for the return value. | None. |
| `E0111` | A `self` or `var self` receiver that is not the first parameter. | None. |
| `E0112` | `var` on a parameter other than the `self` receiver. A parameter is always an immutable binding. | Remove `var`. |
| `E0113` | A `where` clause. Bounds are written inline, as in `fn f<T: Eq>(x: T)`. | None. |
| `E0120` | A missing comma between the declared fields of a struct or the variants of an enum. | Insert the `,`. |
| `E0130` | A glob import such as `use std.sql.*`. Name what you import. | None. |
| `E0131` | A renaming import such as `use std.sql.query as q`. | None. |
| `E0132` | An attribute name outside the fixed set, which holds only `integration`. | None. |
| `E0133` | `unsafe`, which Oska does not have. | None. |
| `E0140` | An or-pattern such as `A \| B`. Write one arm for each. | None. |
| `E0141` | A range pattern such as `1..5`. | None. |
| `E0142` | An `@` binding such as `v @ Some(_)`. | None. |
| `E0143` | A comma between `match` arms. Each arm sits on its own line. | Remove the `,` when the next arm is already on its own line. |
| `E0150` | A `self` or `var self` receiver on a function outside an `impl` or `trait` block. | None. |
| `E0151` | `pub` on an `impl` block. Mark its methods `pub` instead. | Remove `pub`. |
| `E0160` | A string pattern with an interpolation hole. A pattern is a plain literal. | None. |
| `E0161` | A test name with an interpolation hole. | None. |
| `E0162` | `pub` on a trait method. A trait method takes its visibility from the trait. | Remove `pub`. |
| `E0163` | `pub test`. A test is never exported. | Remove `pub`. |
| `E0164` | `pub use`. A re-export is not defined. | Remove `pub`. |
| `E0165` | `#[integration]` on an item other than a `test`. | None. |
| `E0170` | `#[capability]` on an item other than a `struct`, in the prelude. In user code the attribute is `E0132` instead: no program can reach this code, so it has no example. | Remove the attribute. |

### Syntax examples

Each code has a program that fails with exactly that code and the same program fixed. `E0170` has no example: the checker reports it only for the prelude, and user code gets `E0132` instead, so no program can reach it.

#### `E0001`

```oska error=E0001
fn main() {
    let x = $
}
```

```oska fixed=E0001
fn main() {
    let x = 1
}
```

#### `E0002`

```oska error=E0002
fn main() {
    let greeting = "hello
}
```

```oska fixed=E0002
fn main() {
    let greeting = "hello"
}
```

#### `E0003`

```oska error=E0003
fn main() {
    let path = "C:\data"
}
```

```oska fixed=E0003
fn main() {
    let path = "C:\\data"
}
```

#### `E0004`

```oska error=E0004
use std.sql

fn main() {
    let query = sql"SELECT {id}"
}
```

```oska fixed=E0004
use std.sql

fn main() {
    let query = sql"SELECT 1"
}
```

#### `E0005`

```oska error=E0005
fn main() {
    let x = 1;
}
```

```oska fixed=E0005
fn main() {
    let x = 1
}
```

#### `E0100`

```oska error=E0100
fn main() {
    let x = )
}
```

```oska fixed=E0100
fn main() {
    let x = 1
}
```

#### `E0101`

```oska error=E0101
fn first() {} fn second() {}
```

```oska fixed=E0101
fn first() {}
fn second() {}
```

#### `E0102`

```oska error=E0102
fn main() {
    let n = 1
    let text = "next is {n + 1}"
}
```

```oska fixed=E0102
fn main() {
    let n = 1
    let next = n + 1
    let text = "next is {next}"
}
```

#### `E0103`

```oska error=E0103
fn main() {
    {
        let x = 1
    }
}
```

```oska fixed=E0103
fn main() {
    let x = 1
}
```

#### `E0104`

```oska error=E0104
fn main() {
    let x
}
```

```oska fixed=E0104
fn main() {
    let x = 0
}
```

#### `E0105`

```oska error=E0105
fn main() {
    let a = 1
    let ok = 0 < a < 3
}
```

```oska fixed=E0105
fn main() {
    let a = 1
    let ok = 0 < a && a < 3
}
```

#### `E0106`

```oska error=E0106
fn main() {
    var x = 1
    x + 1 = 2
}
```

```oska fixed=E0106
fn main() {
    var x = 1
    x = 2
}
```

#### `E0107`

```oska error=E0107
let x = 1

fn main() {}
```

```oska fixed=E0107
fn main() {
    let x = 1
}
```

#### `E0110`

```oska error=E0110
fn double(n: Int) -> Int
    ensures r > 0
{
    n * 2
}
```

```oska fixed=E0110
fn double(n: Int) -> Int
    ensures |r| r > 0
{
    n * 2
}
```

#### `E0111`

```oska error=E0111
struct Counter { count: Int }

impl Counter {
    fn add(n: Int, self) -> Int {
        self.count + n
    }
}
```

```oska fixed=E0111
struct Counter { count: Int }

impl Counter {
    fn add(self, n: Int) -> Int {
        self.count + n
    }
}
```

#### `E0112`

```oska error=E0112
fn bump(var n: Int) -> Int {
    n + 1
}
```

```oska fixed=E0112
fn bump(n: Int) -> Int {
    n + 1
}
```

#### `E0113`

```oska error=E0113
fn same<T>(a: T, b: T) -> Bool where T: Eq {
    a == b
}
```

```oska fixed=E0113
fn same<T: Eq>(a: T, b: T) -> Bool {
    a == b
}
```

#### `E0120`

```oska error=E0120
struct Point {
    x: Int
    y: Int
}
```

```oska fixed=E0120
struct Point {
    x: Int,
    y: Int
}
```

#### `E0130`

```oska error=E0130
use std.html.*

fn main() {}
```

```oska fixed=E0130
use std.html.{text}

fn main() {}
```

#### `E0131`

```oska error=E0131
use std.html.text as escape

fn main() {}
```

```oska fixed=E0131
use std.html.{text}

fn main() {}
```

#### `E0132`

```oska error=E0132
#[integation]
test "reads a file" {
    t.assert(true)
}
```

```oska fixed=E0132
#[integration]
test "reads a file" {
    t.assert(true)
}
```

#### `E0133`

```oska error=E0133
unsafe fn dangerous() {}
```

```oska fixed=E0133
fn safe() {}
```

#### `E0140`

```oska error=E0140
fn name(n: Int) -> String {
    match n {
        1 | 2 => "small"
        _ => "big"
    }
}
```

```oska fixed=E0140
fn name(n: Int) -> String {
    match n {
        1 => "small"
        2 => "small"
        _ => "big"
    }
}
```

#### `E0141`

```oska error=E0141
fn name(n: Int) -> String {
    match n {
        1..5 => "small"
        _ => "big"
    }
}
```

```oska fixed=E0141
fn name(n: Int) -> String {
    match n {
        _ => if n < 5 { "small" } else { "big" }
    }
}
```

#### `E0142`

```oska error=E0142
fn show(x: Option<Int>) -> Int {
    match x {
        v @ Some(_) => 1
        None => 0
    }
}
```

```oska fixed=E0142
fn show(x: Option<Int>) -> Int {
    match x {
        Some(_) => 1
        None => 0
    }
}
```

#### `E0143`

```oska error=E0143
fn show(x: Option<Int>) -> Int {
    match x {
        Some(_) => 1,
        None => 0
    }
}
```

```oska fixed=E0143
fn show(x: Option<Int>) -> Int {
    match x {
        Some(_) => 1
        None => 0
    }
}
```

#### `E0150`

```oska error=E0150
fn size(self) -> Int {
    1
}
```

```oska fixed=E0150
struct Box { n: Int }

impl Box {
    fn size(self) -> Int {
        1
    }
}
```

#### `E0151`

```oska error=E0151
struct Box { n: Int }

pub impl Box {
    fn size(self) -> Int {
        1
    }
}
```

```oska fixed=E0151
struct Box { n: Int }

impl Box {
    pub fn size(self) -> Int {
        1
    }
}
```

#### `E0160`

```oska error=E0160
fn greet(name: String) -> Int {
    match name {
        "hi {name}" => 1
        _ => 0
    }
}
```

```oska fixed=E0160
fn greet(name: String) -> Int {
    match name {
        "hi" => 1
        _ => 0
    }
}
```

#### `E0161`

```oska error=E0161
test "adds {n}" {
    t.assert(true)
}
```

```oska fixed=E0161
test "adds" {
    t.assert(true)
}
```

#### `E0162`

```oska error=E0162
trait Show {
    pub fn show(self) -> String
}
```

```oska fixed=E0162
trait Show {
    fn show(self) -> String
}
```

#### `E0163`

```oska error=E0163
pub test "adds" {
    t.assert(true)
}
```

```oska fixed=E0163
test "adds" {
    t.assert(true)
}
```

#### `E0164`

```oska error=E0164
pub use std.html

fn main() {}
```

```oska fixed=E0164
use std.html

fn main() {}
```

#### `E0165`

```oska error=E0165
#[integration]
fn setup() {}
```

```oska fixed=E0165
#[integration]
test "setup" {
    t.assert(true)
}
```

## Diagnostic codes

| Code | Meaning | Fix |
| --- | --- | --- |
| `E0200` | Unknown type or trait name, or a qualified path such as `sql.Query` whose first segment is not a module bound by a plain `use`. | Replace the name with the single closest declared name, when one is close enough. |
| `E0201` | A trait where a type is expected (`x: Display`), or a type where a trait is expected (`T: Int`). | None. |
| `E0202` | A type or trait name declared twice in the module, or one that redeclares a prelude name; a free function declared twice, or one that redeclares a prelude function (`dbg`, `todo`); or a method or associated function declared twice for one type, across all of its inherent `impl` blocks. Spans the second declaration's name. | None. |
| `E0210` | A struct or enum whose own fields name a capability-bearing type (a built-in capability, a `cap` type, or a `cap fn`, including inside written type arguments such as `List<Stdout>`) is not declared `cap`. A field typed by a type parameter, as in `struct Box<T> { value: T }`, does not count. Spans the type's name. | Insert `cap` before `struct` or `enum`. |
| `E0211` | A `main` parameter whose type is not a built-in capability or `Args`. Spans the parameter's type. | None. |
| `E0212` | `main` returns something other than `()` or `Result<(), E>` (spans the return type), or `E` has no `impl Display` (spans `E`). | None. |
| `E0213` | A type, enum variant, trait, or type parameter name that does not start with an uppercase letter (ADR 0005). | None. |
| `E0214` | A function, method, field, parameter, or local variable (`let`, `var`, `for`) name that starts with an uppercase letter (ADR 0005). A leading `_` is allowed. Pattern bindings need no check: an uppercase name in a pattern is a variant (`E0250`). | None. |
| `E0215` | A struct or enum with no base case: every way to build it contains itself, as in `struct Loop { me: Loop }`. Recursion through `Option`, `List`, or an enum variant without the type needs no indirection. Each type in a cycle is reported. | None. |
| `E0216` | A call to `dbg(...)` when checking with `--release`. Spans the call. | Replace `dbg(expr)` with `expr`, when it has one argument. |
| `E0217` | A `main` parameter whose type is a directory or file capability: `Dir`, `DirRead`, `DirWrite`, `FileRead`, or `FileWrite`. Only attenuation makes one, with `restrict_to` or `restrict_to_file` on `Fs`, `FsRead`, or `FsWrite`, because the runtime has no root to give. Spans the parameter's type. | None. |
| `E0220` | Type mismatch: an expression's type differs from the type expected there. There are no implicit conversions. | None. |
| `E0221` | Unknown value name, or `self` outside a method with a receiver. | Replace the name with the single closest local, function, or prelude variant, when one is close enough. |
| `E0222` | Integer literal out of range for its type, such as `300` as a `U8`. | None. |
| `E0223` | Operator not defined for its operand's type: arithmetic and ordering need numbers (`+` is not for strings), unary `-` needs a signed number, `..` needs integers. | None. |
| `E0224` | Wrong number of arguments in a call or construction. | None. |
| `E0225` | Argument label that does not match its parameter or field, or a label on a positional field. | Replace the label with the parameter's name, or remove it. |
| `E0226` | Named field constructed without its label. | Insert the field's label. |
| `E0227` | Unknown field, or a method used without calling it. | Replace the field with the single closest field, when one is close enough. |
| `E0228` | Unknown method, variant, or associated function, or an associated function called on a value. | Replace the name with the single closest member, when one is close enough. |
| `E0229` | Assignment to something that is not a `var` binding: a `let` binding, a parameter, or a field of `self` in a method without `var self`. | Change `let` to `var`, or `self` to `var self`. None for parameters. |
| `E0230` | Mutating method (`var self`) called on something that is not a `var` binding. | Change `let` to `var`, or `self` to `var self`. |
| `E0231` | Call of a value that is not a function. | None. |
| `E0232` | `break` or `continue` outside a loop. | None. |
| `E0233` | A type that inference cannot work out, such as `let xs = List.new()` with no later use. | None. |
| `E0234` | A type name used as a value, or a type that cannot be constructed. | None. |
| `E0235` | `for` over a value that is not a `List` or a range. | None. |
| `E0240` | An expression statement whose `Result` value is dropped. Discard one deliberately with `let _ = f()`. Spans the expression. | None. |
| `E0241` | `?` on a value that is neither a `Result` nor an `Option`. Spans the `?` expression. | None. |
| `E0242` | `?` on a `Result` whose error type is not the enclosing function's error type, with no `impl From` to convert it. Spans the `?` expression. | None. |
| `E0243` | `?` in a function (or closure) that does not return the same kind: an `Option` in a function returning `Result` (convert with `.ok_or(err)?`), a `Result` in one returning `Option`, or either in one returning neither. Spans the `?` expression. | None. |
| `E0250` | A pattern that names no variant or struct. Under ADR 0005 an uppercase name in a pattern is always a fieldless variant, never a binding, so a misspelled `Nnoe` is an error rather than a catch-all. Bare names are the prelude's variants (`None`, `Some`, `Ok`, `Err`); other variants are written `Enum.Variant`. Spans the name. | Replace the name with the single closest variant, qualified when it belongs to a user enum (`Empty` becomes `Shape.Empty`). |
| `E0251` | A constructor pattern with the wrong number of fields, such as `Some` or `Shape.Square(a, b)`. Spans the pattern. | None. |
| `E0252` | A named field matched without its label (`Shape.Circle(r)`) or with the wrong one, or a label on a positional field. | Insert, correct, or remove the label. |
| `E0253` | A `match` that is not exhaustive, checked with the pattern-usefulness algorithm through nested constructor patterns. Guarded arms never count. `Int` and `String` matches need a `_` or binding arm. Spans the scrutinee; the message lists the missing patterns. | Add an arm for each missing pattern after the last arm, with `todo()` as its body. |
| `E0254` | An unreachable arm: the earlier unguarded arms already match every value it does, as with any arm after a `_` or binding arm. Spans the arm's pattern. | None. |
| `E0260` | A method called on a value of a generic parameter whose bounds provide no method of that name. A generic body is checked once, against its bounds only. Spans the method name. | Add the bound, when exactly one trait has the method. |
| `E0261` | An `impl Trait for Type` that leaves out a required method (one without a default body). Spans the trait in the `impl`. | Insert a stub for each missing method before the `impl`'s closing brace, with the trait's signature (`Self` replaced) and `todo()` as its body. None when a missing method has generic parameters of its own. |
| `E0262` | An `impl` method whose signature differs from the trait's, once `Self` and the trait's arguments are replaced. Receiver, parameter types, and return type must all match. Spans the method name. | None. |
| `E0263` | An `impl Trait` method that the trait does not declare. Spans the method name. | None. |
| `E0264` | A method call that two or more of the receiver's traits (or a parameter's bounds) provide, when the type's own `impl` does not. Spans the method name. Call it qualified instead: `Display.to_string(x)`. | None. |
| `E0265` | A type that does not implement a trait it must: `==` and `!=` need `Eq`; a call must satisfy the callee's bounds; a qualified call `Trait.method(x)` needs `x`'s type to implement `Trait`; an interpolation hole needs `Display`. Spans the operator expression, the call, or the hole. | On a generic parameter of the enclosing function, add the bound. |
| `E0270` | A `cap fn` used where a plain `fn` is expected: a closure that captures a capability-bearing value (a built-in capability, a `cap` type, a `cap fn`, or a container of one, including a capability-bearing `self`), or a `cap fn` value passed on. The rule is part of type compatibility, so it holds on every route: arguments, returns, assignments, `?`, generic instantiation, and inside type arguments such as `Option<cap fn()>` or `List<cap fn()>`, which never become `Option<fn()>` or `List<fn()>`. A plain `fn` may always be used as a `cap fn`. | None. |
| `E0271` | Construction of a prelude struct with private fields, such as a built-in capability (`Stdout(handle: 1)`) or `Args`. Capabilities cannot be forged; they come from `main`. | None. |
| `E0272` | Reading a private field of a prelude type, such as `out.handle`. | None. |
| `E0273` | A call of `read_to_string`, `list_dir`, `write`, or `create_dir` on `Fs`, `FsRead`, or `FsWrite`. These types only attenuate, so a file is read or written through a directory capability (`restrict_to`) or a file capability (`restrict_to_file`). The message shows the shape of the fix, such as `fs.restrict_to("<root>").read_to_string(path)`. The root is the programmer's to choose, so there is no machine fix. Spans the call. | None. |
| `E0280` | A contract clause (`requires` or `ensures`) calls a function that is not pure: one with a capability-bearing parameter or receiver, or a `cap fn` value. A generic call is judged once instantiated: it is impure when any argument, parameter, or receiver type is capability-bearing after its generics are solved, as in `apply(log, out)` with `fn apply<T>(f: fn(T), x: T)` and `out: Stdout`. Spans the call. | None. |
| `E0281` | A `requires` or `ensures` condition whose type is not `Bool`. Spans the condition. | None. |
| `E0290` | Two `impl Trait for Type` that overlap: some instantiation of each implements the same trait, with the same arguments, for the same type, as with `impl<T> Display for Box<T>` and `impl Display for Box<Int>`. An impl that overlaps a prelude impl, such as `impl Eq for Int`, conflicts too. Spans the second impl's trait. | None. |
| `E0291` | An inherent `impl` for a prelude type, such as `impl FsRead { ... }`: only the prelude declares methods on its types. Write a free function, or implement a trait. Spans the `impl`'s type. | None. |
| `E0292` | A `Scope` handle that could outlive its `scope` block: `Scope` in a function's or method's return type, in a struct or enum field's type, or in the type of a `scope` block's value, including inside wrappers such as `Option<Scope>` or `List<Scope>`. `Scope` stays allowed as a parameter type. A closure that captures a `Scope`, or a value holding one, is **scoped** and cannot escape either: it cannot be a `scope` block's value, be returned, be stored in a field, or be passed to a user function's parameter (a prelude or `std` function, such as `spawn` or `map`, accepts it). A `var` declared outside a `scope` block cannot be left holding the handle or such a closure; this is conservative, and also rejects keeping a closure that captures an enclosing block's handle. Nor can a `var` that already holds a handle, of an enclosing block, take one of an inner block: inside a `scope` block, assigning to a `var` (or `self`) from outside it, or calling a mutating method such as `push` on one, is rejected when the value written mentions a local of the block that holds a `Scope`, such as its handle or a closure capturing it. Spans the type, the closure, or the `scope` expression. | None. |
| `E0293` | A call to `todo()` when checking with `--release`. Spans the call. | None. |
| `E0294` | A `use` whose path does not start with `std.`: only the standard library can be imported. Spans the path. | None. |
| `E0295` | A `use std.*` of a module that does not exist. Spans the module path. | Replace the path with the single closest module, when one is close enough. |
| `E0300` | Untrusted text in a structural position: an `Untrusted<String>` used where trusted `String` is required, such as a `String` parameter, field, or return type. Reported instead of `E0220` when trust is the only difference. Like `E0270`, the rule is part of type compatibility, so it holds on every route, including inside type arguments such as `Option<Untrusted<String>>`. Trusted `String` may always be used as `Untrusted<String>`. The message suggests a validator or `.assume_trusted()`. Spans the expression. | None. |
| `E0310` | A sink literal, such as `sql"..."`, whose module is not imported. Spans the tag. | Insert `use std.<module>` at the start of the file. |
| `E0311` | A sink literal whose tag no `std.*` module provides, such as `sqll"..."`. Spans the tag. | Replace the tag with the single closest known tag, when one is close enough. |
| `E0312` | A name that the module does not export, in a `use` brace group (`use std.sql.{Query}`) or a qualified path (`sql.Query`). Spans the name. | Replace the name with the single closest export, when one is close enough. |
| `E0320` | `Untrusted<T>` where `T` is not text, such as `Untrusted<Int>`, `Untrusted<List<String>>`, or `Untrusted<T>` for a generic `T`. Trust is a flag on text only; every other type is trusted. Text inside a container is written inside it, as in `List<Untrusted<String>>`. Checked in signatures, declarations, and type annotations in bodies. Spans the `Untrusted<...>` type. | None. |
| `E0340` | A module's export written unqualified, such as `Db` after a plain `use std.sql`, which imports only the module. Spans the name. | Qualify it with the module's name: `sql.Db`. |
| `E0341` | Text where a sink literal type is required, such as a `String`, an `Untrusted<String>`, a string literal, or an interpolated string passed to `Db.query` as query text. Only a `sql"..."` literal makes an `SqlLiteral`, so SQL can't be built from text. Reported instead of `E0220`. Spans the expression. | None. |
| `E0350` | A `Secret` interpolated into a string, such as `"Bearer {key}"`, or a value that holds one anywhere inside, such as a `List<Secret<String>>`, an `Option<Secret<String>>`, or a struct with a `Secret` field. Spans the hole, braces included. | None. |
| `E0351` | A `Secret` compared with `==` or `!=`, on either side. Spans the comparison. Also reported, without a fix, where a `Secret` must be `Eq`: an `Eq` bound, or a call to `.eq`. | Rewrite `a == b` to `a.constant_time_eq(b)` and `a != b` to `!a.constant_time_eq(b)`, with the secret as the receiver (parenthesized when needed). When both sides are secrets, it rewrites to `a.constant_time_eq_secret(b)` instead, so nothing is exposed. |
| `E0352` | A `Secret` used where it must be `Display`: a `Display` bound, or a call to `.to_string()`. Spans the call, or the method name. | None. |
| `E0360` | A name declared by an imported `std` module but not exported by it, written unqualified. Spans the name. No `std` module declares a private name yet: no program can reach this code, so it has no example. | None: the module keeps it private. |

### Checker examples

Each code has a program that fails with exactly that code and the same program fixed. `E0360` has no example: no `std` module declares a private name yet, so no program can reach it.

#### `E0200`

```oska error=E0200
fn greet(name: Strng) {}
```

```oska fixed=E0200
fn greet(name: String) {}
```

#### `E0201`

```oska error=E0201
fn show(x: Display) {}
```

```oska fixed=E0201
fn show<T: Display>(x: T) {}
```

#### `E0202`

```oska error=E0202
fn helper() {}

fn helper() {}
```

```oska fixed=E0202
fn helper() {}

fn other_helper() {}
```

#### `E0210`

```oska error=E0210
struct Logger { out: Stdout }
```

```oska fixed=E0210
cap struct Logger { out: Stdout }
```

#### `E0211`

```oska error=E0211
fn main(count: Int) {}
```

```oska fixed=E0211
fn main(io: Stdout) {}
```

#### `E0212`

```oska error=E0212
fn main() -> Int {
    0
}
```

```oska fixed=E0212
fn main() {}
```

#### `E0213`

```oska error=E0213
struct point { x: Int }
```

```oska fixed=E0213
struct Point { x: Int }
```

#### `E0214`

```oska error=E0214
fn Double(n: Int) -> Int {
    n * 2
}
```

```oska fixed=E0214
fn double(n: Int) -> Int {
    n * 2
}
```

#### `E0215`

```oska error=E0215
struct Loop { me: Loop }
```

```oska fixed=E0215
struct Node { next: Option<Node> }
```

#### `E0216`

```oska error=E0216 release
fn main() {
    let x = dbg(1)
}
```

```oska fixed=E0216 release
fn main() {
    let x = 1
}
```

#### `E0217`

```oska error=E0217
fn main(docs: DirRead) {}
```

```oska fixed=E0217
fn main(fs: FsRead) {
    let docs = fs.restrict_to("docs")
}
```

#### `E0220`

```oska error=E0220
fn main() {
    let x: Int = "one"
}
```

```oska fixed=E0220
fn main() {
    let x: Int = 1
}
```

#### `E0221`

```oska error=E0221
fn main() {
    let count = 1
    let total = cuont + 1
}
```

```oska fixed=E0221
fn main() {
    let count = 1
    let total = count + 1
}
```

#### `E0222`

```oska error=E0222
fn main() {
    let x: U8 = 300
}
```

```oska fixed=E0222
fn main() {
    let x: U8 = 200
}
```

#### `E0223`

```oska error=E0223
fn main() {
    let text = "a" + "b"
}
```

```oska fixed=E0223
fn main() {
    let a = "a"
    let b = "b"
    let text = "{a}{b}"
}
```

#### `E0224`

```oska error=E0224
fn add(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let n = add(1)
}
```

```oska fixed=E0224
fn add(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let n = add(1, 2)
}
```

#### `E0225`

```oska error=E0225
fn add(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let n = add(x: 1, b: 2)
}
```

```oska fixed=E0225
fn add(a: Int, b: Int) -> Int {
    a + b
}

fn main() {
    let n = add(a: 1, b: 2)
}
```

#### `E0226`

```oska error=E0226
struct Point { x: Int, y: Int }

fn main() {
    let p = Point(1, 2)
}
```

```oska fixed=E0226
struct Point { x: Int, y: Int }

fn main() {
    let p = Point(x: 1, y: 2)
}
```

#### `E0227`

```oska error=E0227
struct Point { x: Int, y: Int }

fn main() {
    let p = Point(x: 1, y: 2)
    let z = p.z
}
```

```oska fixed=E0227
struct Point { x: Int, y: Int }

fn main() {
    let p = Point(x: 1, y: 2)
    let z = p.x
}
```

#### `E0228`

```oska error=E0228
fn main() {
    let t = "  hi ".trimm()
}
```

```oska fixed=E0228
fn main() {
    let t = "  hi ".trim()
}
```

#### `E0229`

```oska error=E0229
fn main() {
    let x = 1
    x = 2
}
```

```oska fixed=E0229
fn main() {
    var x = 1
    x = 2
}
```

#### `E0230`

```oska error=E0230
struct Counter { count: Int }

impl Counter {
    fn tick(var self) {
        self.count = self.count + 1
    }
}

fn main() {
    let c = Counter(count: 0)
    c.tick()
}
```

```oska fixed=E0230
struct Counter { count: Int }

impl Counter {
    fn tick(var self) {
        self.count = self.count + 1
    }
}

fn main() {
    var c = Counter(count: 0)
    c.tick()
}
```

#### `E0231`

```oska error=E0231
fn main() {
    let x = 1
    x()
}
```

```oska fixed=E0231
fn main() {
    let f = || 1
    let y = f()
}
```

#### `E0232`

```oska error=E0232
fn main() {
    break
}
```

```oska fixed=E0232
fn main() {
    while true {
        break
    }
}
```

#### `E0233`

```oska error=E0233
fn main() {
    let xs = List.new()
}
```

```oska fixed=E0233
fn main() {
    let xs: List<Int> = List.new()
}
```

#### `E0234`

```oska error=E0234
fn main() {
    let x = Int
}
```

```oska fixed=E0234
fn main() {
    let x = 1
}
```

#### `E0235`

```oska error=E0235
fn main() {
    for i in 5 {
        let j = i
    }
}
```

```oska fixed=E0235
fn main() {
    for i in 0..5 {
        let j = i
    }
}
```

#### `E0240`

```oska error=E0240
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hi")
    Ok(())
}
```

```oska fixed=E0240
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hi")?
    Ok(())
}
```

#### `E0241`

```oska error=E0241
fn first() -> Option<Int> {
    let n = 1?
    Some(n)
}
```

```oska fixed=E0241
fn first() -> Option<Int> {
    let n = 1
    Some(n)
}
```

#### `E0242`

```oska error=E0242
fn parse() -> Result<Int, String> {
    Err("bad")
}

fn run() -> Result<Int, Int> {
    let n = parse()?
    Ok(n)
}
```

```oska fixed=E0242
fn parse() -> Result<Int, String> {
    Err("bad")
}

fn run() -> Result<Int, String> {
    let n = parse()?
    Ok(n)
}
```

#### `E0243`

```oska error=E0243
fn double(x: Option<Int>) -> Result<Int, String> {
    let n = x?
    Ok(n * 2)
}
```

```oska fixed=E0243
fn double(x: Option<Int>) -> Result<Int, String> {
    let n = x.ok_or("missing")?
    Ok(n * 2)
}
```

#### `E0250`

```oska error=E0250
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
        Nnoe => 0
    }
}
```

```oska fixed=E0250
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
        None => 0
    }
}
```

#### `E0251`

```oska error=E0251
fn count(x: Option<Int>) -> Int {
    match x {
        Some => 1
        None => 0
    }
}
```

```oska fixed=E0251
fn count(x: Option<Int>) -> Int {
    match x {
        Some(_) => 1
        None => 0
    }
}
```

#### `E0252`

```oska error=E0252
enum Shape {
    Circle { radius: Int },
    Empty,
}

fn size(s: Shape) -> Int {
    match s {
        Shape.Circle(r) => r
        Shape.Empty => 0
    }
}
```

```oska fixed=E0252
enum Shape {
    Circle { radius: Int },
    Empty,
}

fn size(s: Shape) -> Int {
    match s {
        Shape.Circle(radius: r) => r
        Shape.Empty => 0
    }
}
```

#### `E0253`

```oska error=E0253
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
    }
}
```

```oska fixed=E0253
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
        None => 0
    }
}
```

#### `E0254`

```oska error=E0254
fn count(x: Option<Int>) -> Int {
    match x {
        _ => 0
        Some(n) => n
    }
}
```

```oska fixed=E0254
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
        _ => 0
    }
}
```

#### `E0260`

```oska error=E0260
trait Measure {
    fn size(self) -> Int
}

fn total<T>(x: T) -> Int {
    x.size()
}
```

```oska fixed=E0260
trait Measure {
    fn size(self) -> Int
}

fn total<T: Measure>(x: T) -> Int {
    x.size()
}
```

#### `E0261`

```oska error=E0261
trait Shape {
    fn area(self) -> Int
    fn name(self) -> String
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
}
```

```oska fixed=E0261
trait Shape {
    fn area(self) -> Int
    fn name(self) -> String
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
    fn name(self) -> String {
        "square"
    }
}
```

#### `E0262`

```oska error=E0262
trait Shape {
    fn area(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> String {
        "big"
    }
}
```

```oska fixed=E0262
trait Shape {
    fn area(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
}
```

#### `E0263`

```oska error=E0263
trait Shape {
    fn area(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
    fn name(self) -> String {
        "square"
    }
}
```

```oska fixed=E0263
trait Shape {
    fn area(self) -> Int
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> Int {
        self.side * self.side
    }
}

impl Square {
    fn name(self) -> String {
        "square"
    }
}
```

#### `E0264`

```oska error=E0264
trait Short {
    fn describe(self) -> String
}

trait Long {
    fn describe(self) -> String
}

struct Cat { name: String }

impl Short for Cat {
    fn describe(self) -> String {
        "cat"
    }
}

impl Long for Cat {
    fn describe(self) -> String {
        "a cat"
    }
}

fn show(c: Cat) -> String {
    c.describe()
}
```

```oska fixed=E0264
trait Short {
    fn describe(self) -> String
}

trait Long {
    fn describe(self) -> String
}

struct Cat { name: String }

impl Short for Cat {
    fn describe(self) -> String {
        "cat"
    }
}

impl Long for Cat {
    fn describe(self) -> String {
        "a cat"
    }
}

fn show(c: Cat) -> String {
    Short.describe(c)
}
```

#### `E0265`

```oska error=E0265
struct Point { x: Int }

fn main() {
    let a = Point(x: 1)
    let b = Point(x: 1)
    let same = a == b
}
```

```oska fixed=E0265
struct Point { x: Int }

fn main() {
    let a = Point(x: 1)
    let b = Point(x: 1)
    let same = a.x == b.x
}
```

#### `E0270`

```oska error=E0270
fn apply(f: fn()) {
    f()
}

fn main(io: Stdout) {
    apply(|| {
        let _ = io.println("hi")
    })
}
```

```oska fixed=E0270
fn apply(f: cap fn()) {
    f()
}

fn main(io: Stdout) {
    apply(|| {
        let _ = io.println("hi")
    })
}
```

#### `E0271`

```oska error=E0271
fn main() {
    let out = Stdout(handle: 1)
}
```

```oska fixed=E0271
fn main(out: Stdout) {}
```

#### `E0272`

```oska error=E0272
fn main(out: Stdout) {
    let h = out.handle
}
```

```oska fixed=E0272
fn main(out: Stdout) {
    let _ = out.println("hi")
}
```

#### `E0273`

```oska error=E0273
fn count(fs: FsRead, path: Path) -> Result<Int, IoError> {
    Ok(fs.read_to_string(path)?.len())
}
```

```oska fixed=E0273
fn count(fs: FsRead, path: Path) -> Result<Int, IoError> {
    Ok(fs.restrict_to("/srv/app").read_to_string(path)?.len())
}
```

#### `E0280`

```oska error=E0280
fn ready(io: Stdout) -> Bool {
    true
}

fn run(io: Stdout) -> Int
    requires ready(io)
{
    1
}
```

```oska fixed=E0280
fn ready(n: Int) -> Bool {
    n > 0
}

fn run(n: Int) -> Int
    requires ready(n)
{
    1
}
```

#### `E0281`

```oska error=E0281
fn run(n: Int) -> Int
    requires n
{
    n
}
```

```oska fixed=E0281
fn run(n: Int) -> Int
    requires n > 0
{
    n
}
```

#### `E0290`

```oska error=E0290
struct Box<T> { value: T }

impl<T> Display for Box<T> {
    fn to_string(self) -> String {
        "box"
    }
}

impl Display for Box<Int> {
    fn to_string(self) -> String {
        "int box"
    }
}
```

```oska fixed=E0290
struct Box<T> { value: T }

impl<T> Display for Box<T> {
    fn to_string(self) -> String {
        "box"
    }
}
```

#### `E0291`

```oska error=E0291
impl FsRead {
    fn size(self) -> Int {
        1
    }
}
```

```oska fixed=E0291
fn size(fs: FsRead) -> Int {
    1
}
```

#### `E0292`

```oska error=E0292
fn leak() -> Scope {
    scope |s| { s }
}
```

```oska fixed=E0292
fn work() -> Int {
    1
}

fn run() -> Int {
    scope |s| {
        let task = s.spawn(|| work())
        task.join()
    }
}
```

#### `E0293`

```oska error=E0293 release
fn main() {
    todo()
}
```

```oska fixed=E0293 release
fn main() {}
```

#### `E0294`

```oska error=E0294
use mylib.helpers

fn main() {}
```

```oska fixed=E0294
use std.html

fn main() {}
```

#### `E0295`

```oska error=E0295
use std.htm

fn main() {}
```

```oska fixed=E0295
use std.html

fn main() {}
```

#### `E0300`

```oska error=E0300
fn greet(name: String) -> String {
    "Hello, {name}"
}

fn show(user: Untrusted<String>) -> String {
    greet(user)
}
```

```oska fixed=E0300
fn greet(name: Untrusted<String>) -> Untrusted<String> {
    "Hello, {name}"
}

fn show(user: Untrusted<String>) -> Untrusted<String> {
    greet(user)
}
```

#### `E0310`

```oska error=E0310
fn main() {
    let query = sql"SELECT 1"
}
```

```oska fixed=E0310
use std.sql

fn main() {
    let query = sql"SELECT 1"
}
```

#### `E0311`

```oska error=E0311
use std.sql

fn main() {
    let query = sqll"SELECT 1"
}
```

```oska fixed=E0311
use std.sql

fn main() {
    let query = sql"SELECT 1"
}
```

#### `E0312`

```oska error=E0312
use std.sql.{SqlLitreal}

fn main() {}
```

```oska fixed=E0312
use std.sql.{SqlLiteral}

fn main() {}
```

#### `E0320`

```oska error=E0320
fn count(n: Untrusted<Int>) {}
```

```oska fixed=E0320
fn count(n: Int) {}
```

#### `E0340`

```oska error=E0340
use std.sql

fn run(q: SqlLiteral) {}
```

```oska fixed=E0340
use std.sql

fn run(q: sql.SqlLiteral) {}
```

#### `E0341`

```oska error=E0341
use std.sql

fn run(db: sql.Db, id: Untrusted<String>) {
    let rows = db.query("SELECT * FROM users", [id])
}
```

```oska fixed=E0341
use std.sql

fn run(db: sql.Db, id: Untrusted<String>) {
    let rows = db.query(sql"SELECT * FROM users WHERE id = ?", [id])
}
```

#### `E0350`

```oska error=E0350
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let header = "Bearer {key}"
}
```

```oska fixed=E0350
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let value = key.expose()
    let header = "Bearer {value}"
}
```

#### `E0351`

```oska error=E0351
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let ok = key == "hunter2"
}
```

```oska fixed=E0351
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let ok = key.constant_time_eq("hunter2")
}
```

#### `E0352`

```oska error=E0352
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let text = key.to_string()
}
```

```oska fixed=E0352
fn main(env: Env) {
    let key = env.secret("API_KEY")
    let text = key.expose()
}
```
