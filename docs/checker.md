# Oska checker report

`oskac check <file.osk> [--format=json] [--release]` type-checks one file against the embedded prelude. The rules it enforces are in the "Type checking" section of [`design.md`](design.md). This page describes what it prints.

The exit code is non-zero when there are diagnostics. Without `--format=json`, diagnostics go to stderr and the authority report to stdout, one function per line, such as `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`, followed by one line per audit site, such as `audit: assume_trusted (bytes 52..73)`.

Type and trait names in signatures and declarations are resolved, declarations are checked (`cap`, `main`, capitalization, recursive types, and where a `Scope` may appear), functions, methods, and impls are checked for duplicates, and under `--release` every body is scanned for `dbg(...)` and `todo()`. A `use` must name a `std.*` module that exists (`E0294`, `E0295`); see [`std` modules](#std-modules) for which do. Function, method, constant, and test bodies are type-checked with local, bidirectional inference: `let` types come from initializers, the expected type flows into literals, generic constructors, and closures, and an expression that fails gets an error type so it causes only one diagnostic.

Text carries a trust flag (ADR 0006). `Untrusted<String>` is untrusted text; plain `String` is trusted. Text from outside the program is untrusted: `FsRead.read_to_string`, `Stdin.read_line`, `Env.get`, and `Args.get` return it. Trusted text may be used wherever untrusted text is accepted, so a data position such as `Stdout.println` or `Path.from` takes both; untrusted text is never accepted where trusted `String` is required (`E0300`). A validator vouches for text it has checked with `.assume_trusted()`, which returns trusted `String`; each call is a trust assertion, listed in the report's `audit` array. `Untrusted<...>` marks text only (`E0320`).

`Env.secret` returns a `Secret<String>`, which code cannot construct itself (`E0271`). A secret cannot be interpolated, nor can a value that holds one (`E0350`). It is not `Eq`, so `==` and `!=` on one are errors (`E0351`); compare it with `.constant_time_eq(text)`, which takes trusted or untrusted text, or with another secret through `.constant_time_eq_secret(other)`, which exposes neither. It is not `Display`, so it cannot be printed (`E0352`). The only way to read its value is `.expose()`, which returns trusted text; each call is a secret exposure, listed in the report's `audit` array.

Untrusted-ness carries through every route. Prelude text operations are trust-polymorphic: the prelude marks each with `#[follows_trust]`, an attribute only the prelude may write. Each call gets a fresh trust variable that its receiver and text arguments join into, and every text type in its result has that trust, so `untrusted.trim()` and `untrusted.lines()` are untrusted while `untrusted.len()` and `untrusted.contains("x")` are trusted `Int` and `Bool`. Any other function's receiver is checked like an argument. An interpolated string is untrusted when text in any hole is, and every hole must be `Display` (`E0265`). A generic function or trait method follows the trust of its type arguments: when the result holds a fixed `String` and what any generic (or `Self`, in a qualified trait call) is instantiated with holds untrusted text, that text in the result is untrusted, so `fn show<T: Display>(x: T) -> String` cannot launder an untrusted `x`. An impl method may return trusted text where its trait declares `Untrusted<String>`. The `message` of `IoError`, `SqlError`, and `ProcError` is untrusted. `==` and `!=` compare text of either trust, and a trait impl for `String` covers both. Where values meet, trusted and untrusted text join to untrusted rather than conflicting: `if` and `match` branches, the arguments of a generic function for one type parameter, and the elements of a list. Inference variables are solved to trust variables, and the checker keeps every flow between them, so text that has already been used where trusted `String` is required rejects untrusted text that arrives later, as with `names.push(arg)` after `greet(names.get(0)?)`.

Contract clauses are type-checked but not evaluated: `requires` and `ensures` conditions must be `Bool`, they see the parameters (and `self`), `ensures |r|` binds `r` to the return type, and a contract may call only pure functions (`E0280`). A trait method's contract is checked even without a body.

A `test` body sees one built-in value, `t`, of the prelude type `TestContext`. Its fields are stand-in capabilities: `t.fs` (an in-memory `Fs`), `t.clock` (a fixed `Clock`), `t.random` (a seeded `Random`), and `t.stdout` (a recording `Stdout`). Its methods are the assertions `t.assert(cond)`, `t.assert_eq(actual, expected)`, `t.assert_ne(actual, other)`, and `t.fail(message)`. An `#[integration]` test gets the same `t`; the runtime decides which capabilities are real. A test produces no value, so its last expression is a statement like the others.

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
  ]
}
```

- **`diagnostics`**, in source order. Each uses the same object as parser diagnostics (see [`ast.md`](ast.md)): `code`, `message`, `span` (byte offsets), and `fix` when the repair is unambiguous. A parse error is reported alone, with empty `functions` and `audit`.
- **`functions`**: one entry per function and `impl` method, in source order. The report is produced even when there are diagnostics.
  - `name`: the function's name, or `Type.method` for a method.
  - `authority`: the built-in capabilities the function can reach, in order of first appearance, without repeats. `cap` types are expanded through their fields, and type arguments are expanded, so `List<Stdout>` reaches `Stdout`. Attenuated capabilities keep their names, such as `FsRead`.
  - `via`: each capability-bearing parameter and the capabilities it supplies, in parameter order. A method's receiver appears as `self`. A `cap fn` parameter is listed with no capabilities, because a closure's capabilities are not written in its type; it still makes the function impure.
  - `pure`: `true` when no parameter, the receiver included, is capability-bearing.
- **`audit`**: every **audit site**, in source order. It is always present, and is `[]` when there are none.
  - `kind`: what the call does. `assume_trusted` is a **trust assertion**: `.assume_trusted()` turns untrusted text into trusted text, as a validator does. `expose` is a **secret exposure**: `.expose()` reads a `Secret`'s value.
  - `span`: the whole call expression, such as `s.assume_trusted()`.

## The prelude

The prelude is Oska source, `crates/oska-check/src/prelude.osk`, parsed with `parse_prelude`. It declares the built-in capabilities as `#[capability]` structs with private fields, the core types (`Option`, `Result`, `List`, `Range`, `Path`, `Args`, `IoError`) with the few methods the examples and tests use, structured concurrency (`Scope`, whose `spawn(task: cap fn() -> T)` returns a `Task<T>`, whose `join()` returns `T`; `scope |s| { ... }` binds `s: Scope`), the `dbg` and `todo` functions, `TestContext` (the type of `t` in tests), and the core traits (`Eq`, `Hash`, `Display`, `From`). The primitive types (`Int`, `I8` to `I64`, `U8` to `U64`, `Bool`, `String`) and `()` are built into the checker. The prelude implements `Display` for `String`, `Int`, and `IoError`. User code cannot redeclare any of these names.

### `std` modules

A `std.*` module is also prelude-mode Oska source, `crates/oska-check/src/std/<name>.osk`, registered in `STD_MODULES` in `src/prelude.rs`. Its declarations are in scope only in a module that imports it. `use std.sql` imports the module: code reaches its exports qualified, as types and as values (`sql.Db`, `sql.connect(...)`, `sql.Db.query`), and writing one unqualified is `E0340`. `use std.sql.{Db, connect}` imports those names unqualified and does not bind `sql`. There are no glob imports. A brace group or a qualified path may only name what the module exports (`E0312`). Without the `use`, its names are unknown (`E0200`). A local variable shadows a module name. A name the module declares but does not export is private to it (`E0360`). The user module may declare a name that an imported module also declares, as long as it is not imported unqualified: after `use std.sql`, a user `struct Row` is `Row`, and the module's is `sql.Row`. The modules that exist are:

| Module | Exports | Sink literal |
| --- | --- | --- |
| `std.sql` | `SqlLiteral`, `Db` (`query`), `connect`, `Row` (`get`), `SqlError` | `sql"..."` has type `SqlLiteral` |
| `std.proc` | `run`, `Output` (`status`, `stdout`), `ProcError` | None |
| `std.html` | `Html` (`raw`, `concat`, `render`), `text`, `TrustedHtml` (`new`) | None |

A sink literal's tag names the module that provides it. The literal has the type that module declares for it, and the module must be imported (`E0310`). A tag no module provides is `E0311`. The sink modules are signature-only; their runtime implementations come with the interpreter.

Each sink separates a **data position**, declared `Untrusted<String>`, from a **structural position**, declared `String` or a sink literal type. Untrusted text in a `String` structural position is `E0300`; any text where a sink literal is required is `E0341`.

| Sink | Data position | Structural position |
| --- | --- | --- |
| `std.sql` | `Db.query`'s `params: List<Untrusted<String>>` | `Db.query`'s `query: SqlLiteral`; `connect`'s `url` |
| `std.proc` | `run`'s `args: List<Untrusted<String>>` | `run`'s `program`; `run` also needs `Spawn` |
| `std.html` | `text`'s argument, which is escaped | `TrustedHtml.new`'s `markup` |
| Paths | `Path.from` | `Fs.restrict_to` and `FsRead.restrict_to`'s `dir` |
| Network | `Connection.send`'s `body` | `NetConnect.connect`'s `host` |

Values that come back from a sink are untrusted: `Row.get`, `Output.stdout`, `Html.render`, and `Connection.receive`. `Db` and `Connection` are capability-bearing (`Db` holds a `NetConnect`), and every sink type except the error types has private fields, so code outside the module can't construct one (`E0271`).

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
| `E0280` | A contract clause (`requires` or `ensures`) calls a function that is not pure: one with a capability-bearing parameter or receiver, or a `cap fn` value. A generic call is judged once instantiated: it is impure when any argument, parameter, or receiver type is capability-bearing after its generics are solved, as in `apply(log, out)` with `fn apply<T>(f: fn(T), x: T)` and `out: Stdout`. Spans the call. | None. |
| `E0281` | A `requires` or `ensures` condition whose type is not `Bool`. Spans the condition. | None. |
| `E0290` | Two `impl Trait for Type` that overlap: some instantiation of each implements the same trait, with the same arguments, for the same type, as with `impl<T> Display for Box<T>` and `impl Display for Box<Int>`. An impl that overlaps a prelude impl, such as `impl Eq for Int`, conflicts too. Spans the second impl's trait. | None. |
| `E0291` | An inherent `impl` for a prelude type, such as `impl FsRead { ... }`: only the prelude declares methods on its types. Write a free function, or implement a trait. Spans the `impl`'s type. | None. |
| `E0292` | A `Scope` handle that could outlive its `scope` block: `Scope` in a function's or method's return type, in a struct or enum field's type, or in the type of a `scope` block's value, including inside wrappers such as `Option<Scope>` or `List<Scope>`. `Scope` stays allowed as a parameter type. Spans the type, or the `scope` expression. | None. |
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
| `E0360` | A name declared by an imported `std` module but not exported by it, written unqualified. Spans the name. | None: the module keeps it private. |
