# Oska checker report

`oskac check <file.osk> [--format=json] [--release]` type-checks one file against the embedded prelude. The rules it enforces are in the "Type checking" section of [`design.md`](design.md). This page describes what it prints.

The exit code is non-zero when there are diagnostics. Without `--format=json`, diagnostics go to stderr and the authority report to stdout, one function per line, such as `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`.

Type and trait names in signatures and declarations are resolved, declarations are checked (`cap`, `main`, capitalization, recursive types, and where a `Scope` may appear), functions, methods, and impls are checked for duplicates, and under `--release` every body is scanned for `dbg(...)` and `todo()`. A `use` must name a `std.*` module that exists (`E0294`, `E0295`); see [`std` modules](#std-modules) for which do. Function, method, constant, and test bodies are type-checked with local, bidirectional inference: `let` types come from initializers, the expected type flows into literals, generic constructors, and closures, and an expression that fails gets an error type so it causes only one diagnostic.

Text carries a trust flag (ADR 0006). `Untrusted<String>` is untrusted text; plain `String` is trusted. Text from outside the program is untrusted: `FsRead.read_to_string`, `Stdin.read_line`, `Env.get`, and `Args.get` return it. Trusted text may be used wherever untrusted text is accepted, so a data position such as `Stdout.println` or `Path.from` takes both; untrusted text is never accepted where trusted `String` is required (`E0300`).

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
  ]
}
```

- **`diagnostics`**, in source order. Each uses the same object as parser diagnostics (see [`ast.md`](ast.md)): `code`, `message`, `span` (byte offsets), and `fix` when the repair is unambiguous. A parse error is reported alone, with an empty `functions`.
- **`functions`**: one entry per function and `impl` method, in source order. The report is produced even when there are diagnostics.
  - `name`: the function's name, or `Type.method` for a method.
  - `authority`: the built-in capabilities the function can reach, in order of first appearance, without repeats. `cap` types are expanded through their fields, and type arguments are expanded, so `List<Stdout>` reaches `Stdout`. Attenuated capabilities keep their names, such as `FsRead`.
  - `via`: each capability-bearing parameter and the capabilities it supplies, in parameter order. A method's receiver appears as `self`. A `cap fn` parameter is listed with no capabilities, because a closure's capabilities are not written in its type; it still makes the function impure.
  - `pure`: `true` when no parameter, the receiver included, is capability-bearing.

## The prelude

The prelude is Oska source, `crates/oska-check/src/prelude.osk`, parsed with `parse_prelude`. It declares the built-in capabilities as `#[capability]` structs with private fields, the core types (`Option`, `Result`, `List`, `Range`, `Path`, `Args`, `IoError`) with the few methods the examples and tests use, structured concurrency (`Scope`, whose `spawn(task: cap fn() -> T)` returns a `Task<T>`, whose `join()` returns `T`; `scope |s| { ... }` binds `s: Scope`), the `dbg` and `todo` functions, `TestContext` (the type of `t` in tests), and the core traits (`Eq`, `Hash`, `Display`, `From`). The primitive types (`Int`, `I8` to `I64`, `U8` to `U64`, `Bool`, `String`) and `()` are built into the checker. The prelude implements `Display` for `String`, `Int`, and `IoError`. User code cannot redeclare any of these names.

### `std` modules

A `std.*` module is also prelude-mode Oska source, `crates/oska-check/src/std/<name>.osk`, registered in `STD_MODULES` in `src/prelude.rs`. Its declarations are in scope only in a module that imports it: `use std.sql` and `use std.sql.{SqlLiteral}` both bring all of `std.sql` in, and a brace group may only name what the module exports (`E0312`). Without the `use`, its names are unknown (`E0200`). The modules that exist are:

| Module | Exports | Sink literal |
| --- | --- | --- |
| `std.sql` | `SqlLiteral` | `sql"..."` has type `SqlLiteral` |

A sink literal's tag names the module that provides it. The literal has the type that module declares for it, and the module must be imported (`E0310`). A tag no module provides is `E0311`. The sink modules are signature-only; their runtime implementations come with the interpreter.

## Diagnostic codes

| Code | Meaning | Fix |
| --- | --- | --- |
| `E0200` | Unknown type or trait name. A qualified path such as `sql.Query` is unknown until `std.*` modules exist. | Replace the name with the single closest declared name, when one is close enough. |
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
| `E0265` | A type that does not implement a trait it must: `==` and `!=` need `Eq`; a call must satisfy the callee's bounds; a qualified call `Trait.method(x)` needs `x`'s type to implement `Trait`. Spans the operator expression or the call. | On a generic parameter of the enclosing function, add the bound. |
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
| `E0312` | A name in a `use` brace group that the module does not export, such as `use std.sql.{Query}`. Spans the name. | Replace the name with the single closest export, when one is close enough. |
