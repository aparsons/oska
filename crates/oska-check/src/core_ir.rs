// SPDX-License-Identifier: Apache-2.0
//! The **core IR** export (ADR 0018): a checked program in Oska Core,
//! the subset of Oska that the Lean reference defines, as the versioned
//! JSON form that reference reads.
//!
//! Unlike the checked program, whose shape is internal to the toolchain
//! (ADR 0007), the core IR's shape is stable: any change to it is a new
//! [`CORE_IR_VERSION`]. It is a library export for the toolchain's own
//! tests, not a published format, and no CLI command prints it. The
//! goldens of `tests/core_ir.rs` pin it.
//!
//! # Version 7
//!
//! Oska Core version 7 has functions, `let`, `var` and assignment,
//! `if`, `Int` and `Bool` with their operators, `()`, the value of a
//! block or `if` with no value, and the module's own structs and enums
//! without generics, with their construction, field access and `match`.
//! It has text (`String`, trusted or not, as literals), `Path`, and the
//! capabilities `Stdout`, `Fs`, `FsRead`, `FsWrite`, `Dir`, `DirRead`
//! and `DirWrite`, as opaque values that only `main` receives, with the
//! built-in calls in [`BUILTINS`]: writing a line to stdout, reading
//! and writing a file and creating a directory under a directory
//! capability, attenuation, and `is_ok` and `unwrap_or` on a `Result`.
//! It has closures, which capture by value the slots they read,
//! function types, and calls of a function value. It has `Result<T, E>`
//! whose `T` and `E` hold no capability or function, with `Ok`, `Err`,
//! their patterns, and `?`, which may convert the error with the `from`
//! of an `impl From<X> for T` the checker chose; that `impl` is
//! exported as its `from`, a function named as the checker names it,
//! `<T as From<X>>.from`. It has the methods and associated functions
//! of an inherent `impl` of those structs and enums, named `T.name` as
//! the checker names them, and mutating methods (`var self`), with
//! value semantics. An `IoError` is opaque: its field is outside. It
//! has `scope` blocks, with `spawn` and `join` run as the interpreter
//! runs them in v0, one task at a time in spawn order; a handle, the
//! scope's or a task's, is held only in a `let` in its scope's block.
//! [`export`] refuses any other program, naming the first construct
//! outside it in source order, with its span. It fails closed: a trait,
//! any other `impl` or `const` is refused even when nothing uses it,
//! and so are a generic struct or enum, one with a field whose type is
//! outside Oska Core (refused at its declaration), a `use`, a `test`
//! block, a generic function, a contract, a prelude type such as
//! `Option` (and `?` on one), and any type the checker left unsolved.
//! It also refuses any other built-in call (a mutating one such as
//! `List.push` included), `==` and `!=` on anything but `Int`, `Bool`
//! and `()`, a struct or enum with a capability or function field, a
//! function used as a value, a closure that reads `self` or a `var`,
//! a closure that captures a handle, a `Scope` or `Task<T>` type
//! anywhere but a `let`, a `scope` whose value holds a handle (a task
//! that outlives its scope), a `scope` in the block of another, and a
//! `main` that returns anything but `()` or takes anything but
//! `Stdout`, `Fs`, `FsRead` or `FsWrite`, the capabilities the runtime
//! grants.
//!
//! The text is pretty-printed JSON: two-space indentation, a value that
//! fits in 80 columns on one line, and a final newline. Every node is an
//! object whose first field is `kind`; a field shown with `?` is left out
//! when absent, never written as `null`.
//!
//! ```text
//! program  = {"core_ir_version": 7, "types": [typedecl...],
//!             "functions": [function...]}
//! typedecl = {"kind": "Struct", "name": NAME, "fields": [type...]}
//!          | {"kind": "Enum", "name": NAME, "variants": [variant...]}
//! variant  = {"name": NAME, "fields": [type...]}
//! function = {"name": NAME, "receiver"?: NAME, "params": [param...],
//!             "ret": type, "body": expr}
//! param    = {"type": type, "binds": BOOL}
//! type     = "Int" | "Bool" | "()" | "String" | "Path" | "IoError"
//!          | CAPABILITY | NAME
//!          | {"kind": "Fn", "cap": BOOL, "params": [type...], "ret": type}
//!          | {"kind": "Result", "ok": type, "err": type}
//! lettype  = type | "Scope" | {"kind": "Task", "result": type}
//! expr     = {"kind": "Unit"}
//!          | {"kind": "Int", "value": INTEGER}
//!          | {"kind": "Bool", "value": BOOL}
//!          | {"kind": "Str", "value": STRING}
//!          | {"kind": "Local", "slot": N}
//!          | {"kind": "Var", "var": N}
//!          | {"kind": "Neg", "operand": expr}
//!          | {"kind": "Not", "operand": expr}
//!          | {"kind": "Binary", "op": OP, "lhs": expr, "rhs": expr}
//!          | {"kind": "Call", "function": NAME, "args": [expr...]}
//!          | {"kind": "If", "cond": expr, "then": expr, "else"?: expr}
//!          | {"kind": "Block", "stmts": [stmt...], "value"?: expr}
//!          | {"kind": "Construct", "type": NAME, "variant"?: NAME,
//!             "fields": [expr...]}
//!          | {"kind": "Field", "base": expr, "index": N}
//!          | {"kind": "Match", "scrutinee": expr, "arms": [arm...]}
//!          | {"kind": "Builtin", "name": BUILTIN, "args": [expr...]}
//!          | {"kind": "Closure", "params": [param...], "captures": [N...],
//!             "height": N, "cap": BOOL, "ret": type, "body": expr}
//!          | {"kind": "CallValue", "callee": expr, "args": [expr...]}
//!          | {"kind": "Ok", "value": expr}
//!          | {"kind": "Err", "value": expr}
//!          | {"kind": "Try", "operand": expr, "convert"?: NAME}
//!          | {"kind": "MutatingCall", "function": NAME, "receiver": place,
//!             "args": [expr...]}
//!          | {"kind": "Scope", "body": expr}
//! stmt     = {"kind": "Let", "binds": BOOL, "type": lettype, "value": expr}
//!          | {"kind": "Var", "type": type, "value": expr}
//!          | {"kind": "Assign", "place": place, "value": expr}
//!          | {"kind": "Expr", "value": expr}
//! place    = {"var": N, "fields": [N...]}
//! arm      = {"pattern": pattern, "guard"?: expr, "body": expr}
//! pattern  = {"kind": "Wildcard"}
//!          | {"kind": "Bind", "type": type}
//!          | {"kind": "Unit"}
//!          | {"kind": "Int", "value": INTEGER}
//!          | {"kind": "Bool", "value": BOOL}
//!          | {"kind": "Construct", "type": NAME, "variant"?: NAME,
//!             "fields": [pattern...]}
//!          | {"kind": "Ok", "type": type, "value": pattern}
//!          | {"kind": "Err", "type": type, "value": pattern}
//! ```
//!
//! - `CAPABILITY` is `"Stdout"`, `"Fs"`, `"FsRead"`, `"FsWrite"`,
//!   `"Dir"`, `"DirRead"` or `"DirWrite"`, and `BUILTIN` one of
//!   [`BUILTINS`], the checker's name for a body-less prelude function. A
//!   method's receiver is its first argument. No field of a struct or
//!   enum is a capability or a function, and no payload of a `Result`
//!   type holds one, however deep.
//! - `types` are the module's structs and enums, and `functions` its
//!   functions, each in source order. A `NAME` in a `type` is one of
//!   `types`; a `Call` reaches a function by its `NAME`.
//! - A struct's fields, and an enum variant's, are numbered from 0 in
//!   declaration order: `fields` lists their types, a `Construct` their
//!   values, and a constructor pattern their patterns, in that order, and
//!   a `Field` reads field `index` of a struct. A `Construct` with a
//!   `variant` builds that variant of the enum `type`; without one, the
//!   struct `type`.
//! - Slots number a function's locals from 0, in the order they are
//!   bound: the parameters that bind, then each `let` that binds, as it
//!   runs, and each `Bind` of a `match` arm's pattern, in source order. A
//!   `let` is in scope until the end of its block, and a pattern's slots
//!   in its arm's guard and body. A parameter or `let` named `_` has
//!   `"binds": false` and takes no slot: its value is evaluated and
//!   dropped. A slot's value never changes.
//! - A `var` takes no slot: `Var` binds a var, which the rest of its
//!   block reads with `Var` and changes with `Assign` or `MutatingCall`.
//!   Vars are numbered innermost first: the latest `Var` in scope is var
//!   0, the one before it var 1, and so on. A `var _` is a `Let` that does
//!   not bind. A function or closure sees only its own vars, and a `Let`
//!   copies a var's value: a later change to the var does not reach it.
//! - A `place` is var `var`, or a field within it: `fields` lists the
//!   field indices from the var inwards. `Assign` evaluates `value`, then
//!   writes it at `place`.
//! - A method takes `self` as its first parameter, slot 0, and a `Call`
//!   of it passes the receiver as its first argument. A mutating method
//!   has `receiver`, the struct or enum `var self` is, and its body has
//!   it as its outermost var. `MutatingCall` evaluates `args`, then reads
//!   `receiver` and runs `function` with it as its receiver; when the
//!   body ends, by its value or a `?`, the receiver's value is written
//!   back to the place. A `Call` of a mutating method passes the receiver
//!   first, as a copy, and drops its final value.
//! - A `Closure` is written where `height` slots are in scope, and sees
//!   no var. Its body numbers slots as the enclosing function does
//!   there, then its parameters from `height` on, then its own locals.
//!   `captures` lists, in ascending order, the slots below `height` its
//!   body reads, a closure inside it included. Evaluating it makes a
//!   function value of type `fn(params) -> ret` that holds the values
//!   of those slots, by value. `CallValue` evaluates `callee`, then
//!   `args` left to right, then runs the closure's body with what it
//!   captured at their slots and the arguments at its parameters'.
//!   `cap` is true for a `cap fn`: a plain `fn` (a closure's own `cap`
//!   included) captures no capability-bearing value, which the checker
//!   guarantees.
//! - `Int` is a signed 64-bit integer. A literal is written in decimal,
//!   negative when the source writes `-` before it. `Neg` and the
//!   arithmetic operators panic when the result does not fit, and `/`
//!   and `%` when the divisor is 0: `/` rounds toward zero, and `%` has
//!   the sign of its left operand.
//! - `OP` is the operator as written: `+ - * / %` on `Int`; `< <= > >=`
//!   on `Int`, giving `Bool`; `== !=` on two values of one type, `Int`,
//!   `Bool` or `()`; and `&& || implies` on `Bool`, which evaluate `rhs`
//!   only when the result needs it.
//! - `If` evaluates `then` when `cond` is true and `else` when it is
//!   false. Without `else` it has the value `()`, and drops the value of
//!   `then`. A `Block` evaluates its statements in order, dropping the
//!   value of each `Expr`, then its value, or `()` without one.
//! - Arguments, and a `Construct`'s fields, are evaluated left to right,
//!   before the call or the construction.
//! - `Match` evaluates `scrutinee`, then tries its arms in order: the
//!   first whose pattern matches the value, and whose guard, evaluated
//!   with the pattern's slots bound, is then true, gives the value of its
//!   body. `Wildcard` matches any value, `Bind` any value, which it binds;
//!   a literal an equal value; and a constructor pattern a value of its
//!   variant (or struct) whose fields match its patterns. The checker has
//!   made every `match` exhaustive.
//! - `Builtin` evaluates its arguments left to right, then calls the
//!   prelude function `name`, as the runtime implements it
//!   (`crates/oska-run/src/intrinsics.rs`): `Stdout.println` writes the
//!   line and a line break, `K.restrict_to` attenuates to the directory
//!   it names (beneath the receiver's root, for a directory capability),
//!   `read_only` and `write_only` keep the root and narrow the access,
//!   and `read_to_string`, `write` and `create_dir` act beneath the
//!   capability's root, refusing a path that leaves it with an `Err`.
//! - `Ok` and `Err` build a `Result`, and their patterns, which carry the
//!   `Result` type matched, match its variant whose payload matches
//!   `value`. `Try` evaluates `operand`, a `Result`: `Ok(v)` gives `v`;
//!   `Err(e)` calls the function `convert` on `e`, when there is one, and
//!   then returns `Err` of the result from the innermost function or
//!   closure, whose result type is a `Result`.
//! - `main` takes only capabilities the runtime grants: `Stdout`, and
//!   `Fs`, `FsRead` or `FsWrite`, which reach anywhere until attenuated.
//! - `Scope` evaluates `body` with the scope's handle in the next slot.
//!   `Scope.spawn(s, task)` queues a call of `task`, a closure of no
//!   parameter, on the scope and gives its task's handle, of type
//!   `Task<T>` for the closure's result type `T`; nothing runs yet.
//!   `Task.join(t)` runs the scope's queued tasks, oldest first, up to
//!   and including `t`, and gives `t`'s result; a task that already ran
//!   gives it at once. When `body` ends, by its value or a `?`, the tasks
//!   still queued run in spawn order, then the scope gives `body`'s value
//!   or passes the `?` on. A panic in `body` ends the scope at once, and
//!   its queued tasks never run; a panic in a task is the scope's. A
//!   handle is held only in a `Let` in its scope's body, outside any
//!   closure, so `spawn` and `join` always reach the innermost running
//!   scope, and the scope's value holds no handle.
//!
//! # Adding a construct
//!
//! A construct joins Oska Core in a new version: bump [`CORE_IR_VERSION`],
//! extend the grammar above and [`export`], refresh the goldens with
//! `UPDATE_GOLDEN=1`, and update the Lean model in the same pull request
//! (ADR 0018).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;

use oska_syntax::{json_escape, Span};

use crate::program::{
    Arm, Callee, Closure, Construct, Declaration, DeclarationKind, Expr, ExprKind, Function,
    IntType, Param, Pattern, Place, Program, Stmt, Type, TypeDeclKind,
};

/// The version of the core IR [`export`] writes.
pub const CORE_IR_VERSION: u64 = 7;

/// The built-in calls of Oska Core, by the checker's qualified name: the
/// effects on a capability, attenuation, `Path`, the `Result` of an
/// effect, and a scope's `spawn` and `join`.
pub const BUILTINS: &[&str] = &[
    "Stdout.println",
    "Fs.restrict_to",
    "FsRead.restrict_to",
    "FsWrite.restrict_to",
    "Dir.restrict_to",
    "DirRead.restrict_to",
    "DirWrite.restrict_to",
    "Fs.read_only",
    "Fs.write_only",
    "Dir.read_only",
    "Dir.write_only",
    "DirRead.read_to_string",
    "DirWrite.write",
    "DirWrite.create_dir",
    "Path.from",
    "Path.new",
    "Result.is_ok",
    "Result.unwrap_or",
    "Scope.spawn",
    "Task.join",
];

/// The built-ins that reach the innermost running scope.
const TASK_BUILTINS: &[&str] = &["Scope.spawn", "Task.join"];

/// The capabilities of Oska Core, by the name of their type.
const CAPABILITIES: &[&str] = &[
    "Stdout", "Fs", "FsRead", "FsWrite", "Dir", "DirRead", "DirWrite",
];

/// The capabilities `main` may take: the ones the runtime grants.
const GRANTED: &[&str] = &["Stdout", "Fs", "FsRead", "FsWrite"];

/// Why a program has no core IR: its first construct outside Oska Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The construct, such as "the struct `Point`" or "a `while` loop".
    pub construct: String,
    /// The construct's own span: a declaration's whole item, a type as
    /// written, a statement's whole statement, an expression's. A generic
    /// function or a method, refused as a whole, is located at its name.
    pub span: Span,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}..{} is outside Oska Core",
            self.construct, self.span.start, self.span.end
        )
    }
}

/// The core IR of `program`, or the refusal of its first construct
/// outside Oska Core, in source order.
pub fn export(program: &Program) -> Result<String, Refusal> {
    // A type nobody solved has no meaning in Oska Core, wherever it is.
    if let Some(span) = program.unsolved.iter().min_by_key(|s| s.start) {
        return refuse("a type the checker left unsolved", *span);
    }
    // The module's structs and enums without generics: the types a
    // program in Oska Core can name. One with a field outside Oska Core
    // is refused below, at its declaration.
    let core = Core {
        program,
        scope: RefCell::default(),
        types: program
            .declarations
            .iter()
            .filter(|d| matches!(d.kind, DeclarationKind::Struct | DeclarationKind::Enum))
            .filter(|d| {
                program
                    .types
                    .get(&d.name)
                    .is_some_and(|decl| decl.generics.is_empty())
            })
            .map(|d| d.name.clone())
            .collect(),
    };
    let mut refusals: Vec<Refusal> = Vec::new();
    let mut types = Vec::new();
    for declaration in &program.declarations {
        match core.declaration(program, declaration) {
            Ok(Some(json)) => types.push(json),
            Ok(None) => {}
            Err(refusal) => refusals.push(refusal),
        }
    }
    let mut functions: Vec<&Function> = program.functions.values().collect();
    functions.sort_by_key(|f| f.span.start);
    let mut exported = Vec::new();
    for function in functions {
        match core.function(function) {
            Ok(json) => exported.push(json),
            Err(refusal) => refusals.push(refusal),
        }
    }
    refusals.extend(program.imports.iter().map(|(module, span)| Refusal {
        construct: format!("the import of `{module}`"),
        span: *span,
    }));
    refusals.extend(program.tests.iter().map(|test| Refusal {
        construct: "a `test` block".to_string(),
        span: test.span,
    }));
    if let Some(first) = refusals.into_iter().min_by_key(|r| r.span.start) {
        return Err(first);
    }
    let envelope = Json::Obj(vec![
        ("core_ir_version", Json::Int(CORE_IR_VERSION.into())),
        ("types", Json::Arr(types)),
        ("functions", Json::Arr(exported)),
    ]);
    let mut out = String::new();
    envelope.pretty(&mut out, 0);
    out.push('\n');
    Ok(out)
}

fn refuse<T>(construct: impl Into<String>, span: Span) -> Result<T, Refusal> {
    Err(Refusal {
        construct: construct.into(),
        span,
    })
}

fn node(kind: &'static str, mut fields: Vec<(&'static str, Json)>) -> Json {
    fields.insert(0, ("kind", Json::Str(kind.to_string())));
    Json::Obj(fields)
}

/// What the export checks a program's types against: the names of the
/// module's structs and enums that are in Oska Core; and the bindings in
/// scope where it is.
struct Core<'a> {
    program: &'a Program,
    types: BTreeSet<String>,
    scope: RefCell<Scope>,
}

/// What a binding of the checker, which numbers `let`s and `var`s alike
/// as slots, is in core IR, where a `var` takes no slot.
#[derive(Debug, Clone, Copy)]
enum Binding {
    /// A parameter, `let` or pattern binding: this slot.
    Slot(usize),
    /// The `n`th var of the function or closure being exported, counted
    /// from its outermost, 0; a mutating method's receiver is its var 0.
    Var(usize),
    /// A var of the function around a closure, which the closure's body
    /// cannot see.
    Outer,
}

/// The bindings in scope in the function or closure being exported.
#[derive(Debug, Default)]
struct Scope {
    /// By the checker's slot.
    bindings: Vec<Binding>,
    /// The core IR slots in scope.
    slots: usize,
    /// The vars of this function or closure in scope.
    vars: usize,
    /// What `self` is: slot 0 in a method, var 0 in a mutating method.
    receiver: Option<Binding>,
    /// The core IR slots that hold a scope's or a task's handle.
    handles: BTreeSet<usize>,
    /// How many `scope` blocks of this function or closure the export is in.
    scopes: usize,
}

impl Scope {
    /// A scope with `receiver` as `self`, and nothing else yet.
    fn new(receiver: Option<Binding>) -> Self {
        Scope {
            bindings: Vec::new(),
            slots: usize::from(matches!(receiver, Some(Binding::Slot(_)))),
            vars: usize::from(matches!(receiver, Some(Binding::Var(_)))),
            receiver,
            handles: BTreeSet::new(),
            scopes: 0,
        }
    }

    /// The core IR expression that reads `binding`, which
    /// [`Core::binding`] gave, and so is never [`Binding::Outer`].
    fn read(&self, binding: Binding) -> Json {
        match binding {
            Binding::Slot(n) => node("Local", vec![("slot", Json::Int(n as i128))]),
            Binding::Var(level) => node(
                "Var",
                vec![("var", Json::Int((self.vars - 1 - level) as i128))],
            ),
            Binding::Outer => unreachable!("Core::binding refuses a var of an enclosing function"),
        }
    }
}

/// Whether `f` is the `from` of an `impl From<X> for T` whose `T` is not
/// generic, as the checker names it, `<T as From<X>>.from`: the
/// conversion a `?` may make, which the export writes as a function of
/// that name. The checker builds the name with
/// [`crate::program::trait_method`], called from `ImplInfo::method_name`
/// in `items.rs`.
fn is_from_conversion(f: &Function) -> bool {
    match &f.self_ty {
        Some(Type::Named { name, args }) if args.is_empty() && !f.receiver => {
            f.name.starts_with(&format!("<{name} as From<")) && f.name.ends_with(">>.from")
        }
        _ => false,
    }
}

/// Whether `ty` is a scope's or a task's handle, or holds one.
fn holds_handle(ty: &Type) -> bool {
    match ty {
        Type::Named { name, args } => {
            name == "Scope" || name == "Task" || args.iter().any(holds_handle)
        }
        Type::Fn { params, ret, .. } => params.iter().any(holds_handle) || holds_handle(ret),
        _ => false,
    }
}

/// Whether `ty`, a payload of a `Result`, is neither a capability nor a
/// function. A `Result` payload is itself checked when it is exported, so
/// this need not look inside one.
fn holds_no_capability(ty: &Type) -> bool {
    match ty {
        Type::Named { name, .. } => !CAPABILITIES.contains(&name.as_str()),
        Type::Fn { .. } => false,
        _ => true,
    }
}

impl Core<'_> {
    /// Whether `f` is a method or associated function of an inherent
    /// `impl` of a struct or enum of Oska Core, which the checker names
    /// `T.name`; a trait's are named `<T as Trait>.name`.
    fn is_inherent(&self, f: &Function) -> bool {
        match &f.self_ty {
            Some(Type::Named { name, args }) if args.is_empty() && self.types.contains(name) => f
                .name
                .strip_prefix(name.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|method| method.chars().all(|c| c.is_alphanumeric() || c == '_')),
            _ => false,
        }
    }

    /// The core IR of a struct or enum declaration, or the refusal of a
    /// declaration outside Oska Core.
    fn declaration(&self, program: &Program, d: &Declaration) -> Result<Option<Json>, Refusal> {
        let kind = match d.kind {
            DeclarationKind::Struct => "struct",
            DeclarationKind::Enum => "enum",
            DeclarationKind::Trait => return refuse(format!("the trait `{}`", d.name), d.span),
            // An `impl From<X> for T` is its one `from`, exported as a
            // function, and an inherent `impl` of a struct or enum of Oska
            // Core its methods and associated functions; any other `impl`
            // is refused.
            DeclarationKind::Impl => {
                let mut functions = program
                    .functions
                    .values()
                    .filter(|f| d.span.start <= f.span.start && f.span.end <= d.span.end)
                    .peekable();
                if functions.peek().is_some()
                    && functions.all(|f| is_from_conversion(f) || self.is_inherent(f))
                {
                    return Ok(None);
                }
                return refuse(format!("the `impl` of `{}`", d.name), d.span);
            }
            DeclarationKind::Const => return refuse(format!("the constant `{}`", d.name), d.span),
        };
        let Some(decl) = program.types.get(&d.name) else {
            return refuse(format!("the {kind} `{}`", d.name), d.span);
        };
        if !decl.generics.is_empty() {
            return refuse(format!("the generic {kind} `{}`", d.name), d.span);
        }
        // A field has no span of its own here: its type is refused at the
        // declaration.
        let fields = |fields: &[crate::program::Field]| -> Result<Json, Refusal> {
            Ok(Json::Arr(
                fields
                    .iter()
                    .map(|f| self.field_ty(&f.ty, d.span))
                    .collect::<Result<_, _>>()?,
            ))
        };
        Ok(Some(match &decl.kind {
            TypeDeclKind::Struct(fs) => node(
                "Struct",
                vec![("name", Json::Str(d.name.clone())), ("fields", fields(fs)?)],
            ),
            TypeDeclKind::Enum(variants) => node(
                "Enum",
                vec![
                    ("name", Json::Str(d.name.clone())),
                    (
                        "variants",
                        Json::Arr(
                            variants
                                .iter()
                                .map(|v| {
                                    Ok(Json::Obj(vec![
                                        ("name", Json::Str(v.name.clone())),
                                        ("fields", fields(&v.fields)?),
                                    ]))
                                })
                                .collect::<Result<_, Refusal>>()?,
                        ),
                    ),
                ],
            ),
        }))
    }

    fn function(&self, f: &Function) -> Result<Json, Refusal> {
        if f.self_ty.is_some() && !is_from_conversion(f) && !self.is_inherent(f) {
            let kind = if f.receiver {
                "method"
            } else {
                "associated function"
            };
            return refuse(format!("the {kind} `{}`", f.name), f.span);
        }
        if !f.generics.is_empty() {
            return refuse(format!("the generic function `{}`", f.name), f.span);
        }
        // A method takes `self` as its first parameter, slot 0, and a
        // mutating method as its receiver, var 0 of its body.
        let receiver = match &f.self_ty {
            Some(ty) if f.receiver => Some(self.ty(ty, f.span)?),
            _ => None,
        };
        *self.scope.borrow_mut() = Scope::new(match (&receiver, f.mutating) {
            (None, _) => None,
            (Some(_), false) => Some(Binding::Slot(0)),
            (Some(_), true) => Some(Binding::Var(0)),
        });
        // In source order: the parameters, the result type, the contracts.
        let mut params = self.params(&f.params, f.span)?;
        if let (Some(ty), false, Json::Arr(params)) = (&receiver, f.mutating, &mut params) {
            params.insert(
                0,
                Json::Obj(vec![("type", ty.clone()), ("binds", Json::Bool(true))]),
            );
        }
        let ret_span = f.ret_annotation.unwrap_or(f.span);
        let ret = self.ty(&f.ret, ret_span)?;
        if f.name == "main" {
            // The runtime grants `main` its capabilities (the root of
            // authority), and nothing else.
            if let Some(p) = f.params.iter().find(|p| {
                !matches!(&p.ty, Type::Named { name, args }
                    if args.is_empty() && GRANTED.contains(&name.as_str()))
            }) {
                return refuse(
                    format!("a `main` that takes `{}`", p.ty),
                    p.annotation.unwrap_or(f.span),
                );
            }
            if f.ret != Type::Unit {
                return refuse(format!("a `main` that returns `{}`", f.ret), ret_span);
            }
        }
        if let Some(clause) = f
            .contracts
            .requires
            .iter()
            .chain(f.contracts.ensures.iter().map(|e| &e.clause))
            .min_by_key(|c| c.span.start)
        {
            return refuse("a contract", clause.span);
        }
        let mut fields = vec![("name", Json::Str(f.name.clone()))];
        if let (Some(ty), true) = (receiver, f.mutating) {
            fields.push(("receiver", ty));
        }
        fields.extend([
            ("params", params),
            ("ret", ret),
            ("body", self.expr(&f.body)?),
        ]);
        Ok(Json::Obj(fields))
    }

    /// The parameters of a function or closure, each refused at its
    /// annotation, or at `span` when it has none. Each that binds takes
    /// the next slot.
    fn params(&self, params: &[Param], span: Span) -> Result<Json, Refusal> {
        Ok(Json::Arr(
            params
                .iter()
                .map(|p| {
                    let ty = self.ty(&p.ty, p.annotation.unwrap_or(span))?;
                    if p.binds {
                        self.bind_slots(1);
                    }
                    Ok(Json::Obj(vec![
                        ("type", ty),
                        ("binds", Json::Bool(p.binds)),
                    ]))
                })
                .collect::<Result<_, Refusal>>()?,
        ))
    }

    /// Bind the checker's next `n` slots as core IR slots.
    fn bind_slots(&self, n: usize) {
        let mut scope = self.scope.borrow_mut();
        for _ in 0..n {
            let slot = scope.slots;
            scope.bindings.push(Binding::Slot(slot));
            scope.slots += 1;
        }
    }

    /// Bind the checker's next slot as the next var.
    fn bind_var(&self) {
        let mut scope = self.scope.borrow_mut();
        let level = scope.vars;
        scope.bindings.push(Binding::Var(level));
        scope.vars += 1;
    }

    /// Bind the checker's next slot as a core IR slot that holds a
    /// handle.
    fn bind_handle(&self) {
        self.bind_slots(1);
        let mut scope = self.scope.borrow_mut();
        let slot = scope.slots - 1;
        scope.handles.insert(slot);
    }

    /// Run `f`, then leave the bindings it made: a block's, an arm's or
    /// a `scope`'s.
    fn scoped<T>(&self, f: impl FnOnce() -> T) -> T {
        let (bindings, slots, vars, scopes) = {
            let scope = self.scope.borrow();
            (scope.bindings.len(), scope.slots, scope.vars, scope.scopes)
        };
        let result = f();
        let mut scope = self.scope.borrow_mut();
        scope.bindings.truncate(bindings);
        scope.slots = slots;
        scope.vars = vars;
        scope.scopes = scopes;
        scope.handles.retain(|&slot| slot < slots);
        result
    }

    /// The binding of the checker's `slot`, refused at `span` when the
    /// export cannot read it.
    fn binding(&self, slot: usize, span: Span) -> Result<Binding, Refusal> {
        match self.scope.borrow().bindings.get(slot) {
            Some(Binding::Outer) => refuse("a closure that captures a `var`", span),
            Some(binding) => Ok(*binding),
            None => refuse("a local the export cannot number", span),
        }
    }

    /// A place of core IR: a var and the fields within it. Only a `var` is
    /// a place (`E0229`, `E0230`); fail closed if not.
    fn place(&self, place: &Place, span: Span) -> Result<Json, Refusal> {
        let mut fields = Vec::new();
        let mut at = place;
        let root = loop {
            match at {
                Place::Field { base, index } => {
                    fields.push(Json::Int(*index as i128));
                    at = base;
                }
                Place::Local(slot) => break Some(self.binding(*slot, span)?),
                Place::SelfValue => break self.scope.borrow().receiver,
            }
        };
        let Some(Binding::Var(level)) = root else {
            return refuse("a place that is not a `var`", span);
        };
        fields.reverse();
        let var = self.scope.borrow().vars - 1 - level;
        Ok(Json::Obj(vec![
            ("var", Json::Int(var as i128)),
            ("fields", Json::Arr(fields)),
        ]))
    }

    /// A type of Oska Core, or the refusal of another, at `span`.
    fn ty(&self, ty: &Type, span: Span) -> Result<Json, Refusal> {
        if let Type::Fn { cap, params, ret } = ty {
            return Ok(node(
                "Fn",
                vec![
                    ("cap", Json::Bool(*cap)),
                    (
                        "params",
                        Json::Arr(
                            params
                                .iter()
                                .map(|p| self.ty(p, span))
                                .collect::<Result<_, _>>()?,
                        ),
                    ),
                    ("ret", self.ty(ret, span)?),
                ],
            ));
        }
        let name = match ty {
            Type::Unit => "()",
            Type::Bool => "Bool",
            Type::Int(IntType::Int) => "Int",
            Type::String => "String",
            Type::Named { name, args } if args.is_empty() && self.types.contains(name) => name,
            Type::Named { name, args }
                if args.is_empty() && (name == "Path" || CAPABILITIES.contains(&name.as_str())) =>
            {
                name
            }
            Type::Named { name, args } if args.is_empty() && name == "IoError" => name,
            Type::Named { name, .. } if name == "Scope" || name == "Task" => {
                return refuse(format!("the type `{ty}` outside a `let`"), span)
            }
            Type::Named { name, args } if name == "Result" => {
                let [ok, err] = args.as_slice() else {
                    return refuse(format!("the type `{ty}`"), span);
                };
                let payloads_ok = holds_no_capability(ok) && holds_no_capability(err);
                let (ok, err) = (self.ty(ok, span)?, self.ty(err, span)?);
                if !payloads_ok {
                    return refuse(
                        format!(
                            "the type `{ty}`, a `Result` that holds a capability or a function"
                        ),
                        span,
                    );
                }
                return Ok(node("Result", vec![("ok", ok), ("err", err)]));
            }
            Type::Int(_)
            | Type::Float
            | Type::Named { .. }
            | Type::Fn { .. }
            | Type::Param(_)
            | Type::Never => return refuse(format!("the type `{ty}`"), span),
        };
        Ok(Json::Str(name.to_string()))
    }

    /// The type of a `let`: a type of Oska Core, or a scope's or a task's
    /// handle, which only a `let` holds.
    fn let_ty(&self, ty: &Type, span: Span) -> Result<Json, Refusal> {
        match ty {
            Type::Named { name, args } if name == "Scope" && args.is_empty() => {
                Ok(Json::Str(name.clone()))
            }
            Type::Named { name, args } if name == "Task" && args.len() == 1 => {
                Ok(node("Task", vec![("result", self.ty(&args[0], span)?)]))
            }
            // A `Scope` or `Task` with the wrong number of type arguments,
            // which the checker never passes: refused as itself, not as a
            // handle outside a `let`.
            Type::Named { name, .. } if name == "Scope" || name == "Task" => {
                refuse(format!("the type `{ty}`"), span)
            }
            _ => self.ty(ty, span),
        }
    }

    /// The type of a struct or enum field, which is never a capability or
    /// a function, whose value may have captured one.
    fn field_ty(&self, ty: &Type, span: Span) -> Result<Json, Refusal> {
        if matches!(ty, Type::Named { name, .. } if CAPABILITIES.contains(&name.as_str())) {
            return refuse(format!("the capability field type `{ty}`"), span);
        }
        if matches!(ty, Type::Fn { .. }) {
            return refuse(format!("the function field type `{ty}`"), span);
        }
        self.ty(ty, span)
    }

    fn exprs(&self, items: &[Expr]) -> Result<Json, Refusal> {
        Ok(Json::Arr(
            items
                .iter()
                .map(|e| self.expr(e))
                .collect::<Result<_, _>>()?,
        ))
    }

    /// An expression of Oska Core, or the refusal of the first construct in
    /// it that is not. A node is refused before anything inside it, and the
    /// parts of a node in source order, so the first refusal met is the
    /// first in the source.
    fn expr(&self, e: &Expr) -> Result<Json, Refusal> {
        let span = e.span;
        Ok(match &e.kind {
            ExprKind::Unit => node("Unit", Vec::new()),
            ExprKind::Int(value, IntType::Int) => node("Int", vec![("value", Json::Int(*value))]),
            ExprKind::Int(_, int) => {
                return refuse(format!("the integer type `{}`", int.name()), span)
            }
            ExprKind::Float(_) | ExprKind::FloatBinary { .. } | ExprKind::FloatNeg(_) => {
                return refuse("a `Float` (core IR has no Float yet; the Lean model of Float is checked on its own)", span)
            }
            ExprKind::Bool(value) => node("Bool", vec![("value", Json::Bool(*value))]),
            ExprKind::Local(slot) => {
                let binding = self.binding(*slot, span)?;
                self.scope.borrow().read(binding)
            }
            ExprKind::Neg { int, .. } | ExprKind::Binary { int: Some(int), .. }
                if *int != IntType::Int =>
            {
                return refuse(format!("the integer type `{}`", int.name()), span)
            }
            ExprKind::Neg { operand, .. } => node("Neg", vec![("operand", self.expr(operand)?)]),
            ExprKind::BitNot { int, .. } => {
                return refuse(format!("the integer type `{}`", int.name()), span)
            }
            ExprKind::Select { .. } => {
                return refuse("a choice without a branch (`ct_select`)", span)
            }
            ExprKind::Not(operand) => node("Not", vec![("operand", self.expr(operand)?)]),
            ExprKind::Binary { op, lhs, .. }
                if !matches!(lhs.ty, Type::Unit | Type::Bool | Type::Int(IntType::Int)) =>
            {
                return refuse(
                    format!("the operator `{}` on `{}`", op.text(), lhs.ty),
                    span,
                )
            }
            ExprKind::Binary { op, lhs, rhs, .. } => node(
                "Binary",
                vec![
                    ("op", Json::Str(op.text().to_string())),
                    ("lhs", self.expr(lhs)?),
                    ("rhs", self.expr(rhs)?),
                ],
            ),
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => self.call_expr(span, callee, type_args, args)?,
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_expr(cond, then_branch, else_branch.as_deref())?,
            ExprKind::Block { stmts, value } => self.block_expr(stmts, value.as_deref())?,
            ExprKind::Str(value) => node("Str", vec![("value", Json::Str(value.clone()))]),
            ExprKind::Interpolated(_) => return refuse("an interpolated string", span),
            ExprKind::SelfValue => {
                let scope = self.scope.borrow();
                match scope.receiver {
                    Some(binding) => scope.read(binding),
                    None => return refuse("`self`", span),
                }
            }
            ExprKind::List(_) => return refuse("a list", span),
            ExprKind::Construct { ctor, fields } if ctor.ty == "Result" => {
                self.result_expr(e, span, ctor, fields)?
            }
            ExprKind::Construct { ctor, fields } => {
                let mut parts = self.ctor(ctor, span)?;
                parts.push(("fields", self.exprs(fields)?));
                node("Construct", parts)
            }
            ExprKind::Field { base, index } => self.field_expr(span, base, *index)?,
            ExprKind::Match { scrutinee, arms } => self.match_expr(span, scrutinee, arms)?,
            ExprKind::Closure(closure) => self.closure_expr(e, span, closure)?,
            ExprKind::CallValue { callee, args } => node(
                "CallValue",
                vec![("callee", self.expr(callee)?), ("args", self.exprs(args)?)],
            ),
            ExprKind::MutatingCall {
                callee,
                type_args,
                receiver,
                args,
            } => self.mutating_call(span, callee, type_args, receiver, args)?,
            ExprKind::Range { .. } => return refuse("a range", span),
            ExprKind::Try {
                operand,
                convert,
                type_args,
            } => self.try_expr(span, operand, convert, type_args)?,
            ExprKind::FnValue { .. } => return refuse("a function used as a value", span),
            ExprKind::Scope(body) => self.scope_expr(e, span, body)?,
            ExprKind::Unsupported(what) => return refuse(what.clone(), span),
        })
    }

    /// A read of a field of a struct of the module.
    fn field_expr(&self, span: Span, base: &Expr, index: usize) -> Result<Json, Refusal> {
        let json = self.expr(base)?;
        // Only a struct of the module has fields to read: an
        // `IoError`'s are outside Oska Core.
        if !matches!(&base.ty, Type::Named { name, .. } if self.types.contains(name)) {
            self.ty(&base.ty, base.span)?;
            return refuse(format!("a field of `{}`", base.ty), span);
        }
        Ok(node(
            "Field",
            vec![("base", json), ("index", Json::Int(index as i128))],
        ))
    }

    /// An `if`, with or without `else`.
    fn if_expr(
        &self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
    ) -> Result<Json, Refusal> {
        let mut fields = vec![
            ("cond", self.expr(cond)?),
            ("then", self.expr(then_branch)?),
        ];
        if let Some(else_branch) = else_branch {
            fields.push(("else", self.expr(else_branch)?));
        }
        Ok(node("If", fields))
    }

    /// A block: its statements in a scope of their own, then its value.
    fn block_expr(&self, stmts: &[Stmt], value: Option<&Expr>) -> Result<Json, Refusal> {
        self.scoped(|| {
            let stmts = stmts
                .iter()
                .map(|s| self.stmt(s))
                .collect::<Result<Vec<_>, _>>()?;
            let mut fields = vec![("stmts", Json::Arr(stmts))];
            if let Some(value) = value {
                fields.push(("value", self.expr(value)?));
            }
            Ok(node("Block", fields))
        })
    }

    /// A call of a function or built-in.
    fn call_expr(
        &self,
        span: Span,
        callee: &Callee,
        type_args: &[Type],
        args: &[Expr],
    ) -> Result<Json, Refusal> {
        Ok(match callee {
            Callee::Function(name) if type_args.is_empty() => node(
                "Call",
                vec![
                    ("function", Json::Str(name.clone())),
                    ("args", self.exprs(args)?),
                ],
            ),
            Callee::Function(name) => {
                return refuse(format!("a call of the generic function `{name}`"), span)
            }
            Callee::Intrinsic(name)
                if TASK_BUILTINS.contains(&name.as_str()) && self.scope.borrow().scopes == 0 =>
            {
                // A handle is held only in a `scope`'s block, so a
                // checked program cannot reach this guard. It is kept to
                // fail closed.
                return refuse(format!("the built-in `{name}` outside a `scope`"), span);
            }
            Callee::Intrinsic(name) if BUILTINS.contains(&name.as_str()) => node(
                "Builtin",
                vec![
                    ("name", Json::Str(name.to_string())),
                    ("args", self.exprs(args)?),
                ],
            ),
            Callee::Intrinsic(name) => return refuse(format!("the built-in `{name}`"), span),
            Callee::Trait { trait_name, method } => {
                return refuse(format!("the trait method `{trait_name}.{method}`"), span)
            }
        })
    }

    /// `Ok(v)` or `Err(v)`, of a `Result` type of Oska Core.
    fn result_expr(
        &self,
        e: &Expr,
        span: Span,
        ctor: &Construct,
        fields: &[Expr],
    ) -> Result<Json, Refusal> {
        // `Ok(v)` or `Err(v)`, of a `Result` type of Oska Core.
        self.ty(&e.ty, span)?;
        let kind = match (ctor.variant.as_deref(), fields) {
            (Some("Ok"), [_]) => "Ok",
            (Some("Err"), [_]) => "Err",
            _ => return refuse(format!("the constructor `{}`", e.ty), span),
        };
        Ok(node(kind, vec![("value", self.expr(&fields[0])?)]))
    }

    /// A `match`: its scrutinee, then its arms.
    fn match_expr(&self, span: Span, scrutinee: &Expr, arms: &[Arm]) -> Result<Json, Refusal> {
        let json = self.expr(scrutinee)?;
        self.ty(&scrutinee.ty, scrutinee.span)?;
        let arms = arms
            .iter()
            .map(|arm| self.arm(arm, &scrutinee.ty, span))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(node(
            "Match",
            vec![("scrutinee", json), ("arms", Json::Arr(arms))],
        ))
    }

    /// A closure: captures by value, no `var`, no `self`.
    fn closure_expr(&self, e: &Expr, span: Span, closure: &Closure) -> Result<Json, Refusal> {
        if closure.captures_self {
            return refuse("a closure that reads `self`", span);
        }
        // The closure's type is not written: its result type is
        // refused at the closure, before its parameters and body.
        let Type::Fn { cap, ret, .. } = &e.ty else {
            return refuse(format!("a closure of type `{}`", e.ty), span);
        };
        let ret = self.ty(ret, span)?;
        // A closure captures slots, by value, and sees no var: a
        // `var` it reads is refused.
        let captures = closure
            .captures
            .iter()
            .map(|&slot| match self.binding(slot, span)? {
                Binding::Slot(n) if self.scope.borrow().handles.contains(&n) => {
                    refuse("a closure that captures a scope's or a task's handle", span)
                }
                Binding::Slot(n) => Ok(Json::Int(n as i128)),
                Binding::Var(_) => refuse("a closure that captures a `var`", span),
                Binding::Outer => {
                    unreachable!("Core::binding refuses a var of an enclosing function")
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let height = {
            let scope = self.scope.borrow();
            if scope.bindings.len() != closure.height {
                return refuse("a closure the export cannot number", span);
            }
            scope.slots
        };
        let inner = Scope {
            bindings: self
                .scope
                .borrow()
                .bindings
                .iter()
                .map(|b| match b {
                    Binding::Slot(n) => Binding::Slot(*n),
                    Binding::Var(_) | Binding::Outer => Binding::Outer,
                })
                .collect(),
            slots: height,
            vars: 0,
            receiver: None,
            handles: BTreeSet::new(),
            scopes: 0,
        };
        let outer = self.scope.replace(inner);
        let body = self
            .params(&closure.params, span)
            .and_then(|params| Ok((params, self.expr(&closure.body)?)));
        self.scope.replace(outer);
        let (params, body) = body?;
        Ok(node(
            "Closure",
            vec![
                ("params", params),
                ("captures", Json::Arr(captures)),
                ("height", Json::Int(height as i128)),
                ("cap", Json::Bool(*cap)),
                ("ret", ret),
                ("body", body),
            ],
        ))
    }

    /// A call of a mutating method.
    fn mutating_call(
        &self,
        span: Span,
        callee: &Callee,
        type_args: &[Type],
        receiver: &Place,
        args: &[Expr],
    ) -> Result<Json, Refusal> {
        Ok(match callee {
            Callee::Function(name) if type_args.is_empty() => node(
                "MutatingCall",
                vec![
                    ("function", Json::Str(name.clone())),
                    ("receiver", self.place(receiver, span)?),
                    ("args", self.exprs(args)?),
                ],
            ),
            Callee::Function(name) => {
                return refuse(format!("a call of the generic function `{name}`"), span)
            }
            Callee::Intrinsic(name) => return refuse(format!("the built-in `{name}`"), span),
            Callee::Trait { trait_name, method } => {
                return refuse(format!("the trait method `{trait_name}.{method}`"), span)
            }
        })
    }

    /// `?` on a `Result`.
    fn try_expr(
        &self,
        span: Span,
        operand: &Expr,
        convert: &Option<Callee>,
        type_args: &[Type],
    ) -> Result<Json, Refusal> {
        // `?` on a `Result`, converting its error with the `from`
        // the checker chose, if any. `?` on an `Option` is outside
        // Oska Core. A program from source cannot reach this guard:
        // such a `?` needs an `Option` result type, refused before
        // the body. It is kept to fail closed.
        if !matches!(&operand.ty, Type::Named { name, .. } if name == "Result") {
            return refuse(format!("`?` on `{}`", operand.ty), span);
        }
        let mut fields = vec![("operand", self.expr(operand)?)];
        match convert {
            None => {}
            Some(Callee::Function(name)) if type_args.is_empty() => {
                fields.push(("convert", Json::Str(name.clone())));
            }
            Some(_) => return refuse("a `?` that converts with a generic function", span),
        }
        Ok(node("Try", fields))
    }

    /// A `scope` block.
    fn scope_expr(&self, e: &Expr, span: Span, body: &Expr) -> Result<Json, Refusal> {
        if self.scope.borrow().scopes > 0 {
            return refuse("a `scope` in the block of another `scope`", span);
        }
        if holds_handle(&e.ty) {
            return refuse(
                format!("a `scope` whose value, of type `{}`, outlives it", e.ty),
                span,
            );
        }
        self.scoped(|| {
            self.bind_handle();
            self.scope.borrow_mut().scopes += 1;
            Ok(node("Scope", vec![("body", self.expr(body)?)]))
        })
    }

    /// The `type` and `variant` fields of a constructor of Oska Core, or the
    /// refusal of one of another type, at `span`.
    fn ctor(&self, ctor: &Construct, span: Span) -> Result<Vec<(&'static str, Json)>, Refusal> {
        if !self.types.contains(&ctor.ty) {
            let construct = match &ctor.variant {
                Some(variant) => format!("the enum variant `{}.{variant}`", ctor.ty),
                None => format!("the struct `{}`", ctor.ty),
            };
            return refuse(construct, span);
        }
        let mut parts = vec![("type", Json::Str(ctor.ty.clone()))];
        if let Some(variant) = &ctor.variant {
            parts.push(("variant", Json::Str(variant.clone())));
        }
        Ok(parts)
    }

    /// A `match` arm of Oska Core on a value of type `ty`, or the refusal
    /// of the first construct in it that is not. A pattern has no span of
    /// its own: one outside Oska Core is refused at the `match`, `span`.
    fn arm(&self, arm: &Arm, ty: &Type, span: Span) -> Result<Json, Refusal> {
        self.scoped(|| {
            let mut fields = vec![("pattern", self.pattern(&arm.pattern, ty, span)?)];
            self.bind_slots(binds(&arm.pattern));
            if let Some(guard) = &arm.guard {
                fields.push(("guard", self.expr(guard)?));
            }
            fields.push(("body", self.expr(&arm.body)?));
            Ok(Json::Obj(fields))
        })
    }

    /// A pattern of Oska Core on a value of type `ty`, or the refusal of
    /// one that is not. An `Ok` or `Err` pattern carries its `Result` type.
    fn pattern(&self, pattern: &Pattern, ty: &Type, span: Span) -> Result<Json, Refusal> {
        Ok(match pattern {
            Pattern::Wildcard => node("Wildcard", Vec::new()),
            Pattern::Bind(ty) => node("Bind", vec![("type", self.ty(ty, span)?)]),
            Pattern::Literal(literal) => match &literal.kind {
                ExprKind::Unit | ExprKind::Int(_, IntType::Int) | ExprKind::Bool(_) => {
                    self.expr(literal)?
                }
                _ => {
                    // The literal's own refusal, such as "an interpolated string".
                    self.expr(literal)?;
                    return refuse("this literal pattern", literal.span);
                }
            },
            Pattern::Construct { ctor, fields } if ctor.ty == "Result" => {
                let json = self.ty(ty, span)?;
                let (kind, payload) = match (ctor.variant.as_deref(), ty, fields.as_slice()) {
                    (Some("Ok"), Type::Named { args, .. }, [_]) if args.len() == 2 => {
                        ("Ok", &args[0])
                    }
                    (Some("Err"), Type::Named { args, .. }, [_]) if args.len() == 2 => {
                        ("Err", &args[1])
                    }
                    _ => return refuse(format!("a pattern on `{ty}`"), span),
                };
                node(
                    kind,
                    vec![
                        ("type", json),
                        ("value", self.pattern(&fields[0], payload, span)?),
                    ],
                )
            }
            Pattern::Construct { ctor, fields } => {
                let mut parts = self.ctor(ctor, span)?;
                let types = self.field_types(ctor);
                // The checker matches every field; fail closed if not.
                if types.len() != fields.len() {
                    return refuse(format!("a pattern on `{ty}`"), span);
                }
                let fields = fields
                    .iter()
                    .zip(&types)
                    .map(|(p, field)| self.pattern(p, field, span))
                    .collect::<Result<Vec<_>, _>>()?;
                parts.push(("fields", Json::Arr(fields)));
                node("Construct", parts)
            }
        })
    }

    /// The types of the fields of a struct or variant of the module, in
    /// declaration order: none for one it does not declare.
    fn field_types(&self, ctor: &Construct) -> Vec<Type> {
        let fields = match (
            self.program.types.get(&ctor.ty).map(|d| &d.kind),
            &ctor.variant,
        ) {
            (Some(TypeDeclKind::Struct(fields)), None) => fields,
            (Some(TypeDeclKind::Enum(variants)), Some(variant)) => {
                match variants.iter().find(|v| &v.name == variant) {
                    Some(v) => &v.fields,
                    None => return Vec::new(),
                }
            }
            _ => return Vec::new(),
        };
        fields.iter().map(|f| f.ty.clone()).collect()
    }

    /// A statement of Oska Core, or the refusal of the first construct in
    /// it that is not.
    fn stmt(&self, s: &Stmt) -> Result<Json, Refusal> {
        Ok(match s {
            Stmt::Expr(value) => node("Expr", vec![("value", self.expr(value)?)]),
            Stmt::Let {
                binds,
                mutable,
                ty,
                annotation,
                value,
                ..
            } => {
                // A written type comes before the value; an inferred one is
                // the value's. Only a `let` holds a handle.
                let handle = holds_handle(ty);
                let of = |ty, span| {
                    if *mutable {
                        self.ty(ty, span)
                    } else {
                        self.let_ty(ty, span)
                    }
                };
                let (ty, value) = match annotation {
                    Some(annotation) => {
                        let ty = of(ty, *annotation)?;
                        (ty, self.expr(value)?)
                    }
                    None => {
                        let json = self.expr(value)?;
                        (of(ty, value.span)?, json)
                    }
                };
                // A `var` takes no slot, but the next var; one named `_`
                // is a `let` that drops its value.
                if *mutable && *binds {
                    self.bind_var();
                    return Ok(node("Var", vec![("type", ty), ("value", value)]));
                }
                if *binds && handle {
                    self.bind_handle();
                } else if *binds {
                    self.bind_slots(1);
                }
                node(
                    "Let",
                    vec![
                        ("binds", Json::Bool(*binds)),
                        ("type", ty),
                        ("value", value),
                    ],
                )
            }
            Stmt::Assign { place, value, span } => node(
                "Assign",
                vec![
                    ("place", self.place(place, *span)?),
                    ("value", self.expr(value)?),
                ],
            ),
            Stmt::Return { span, .. } => return refuse("`return`", *span),
            Stmt::While { span, .. } => return refuse("a `while` loop", *span),
            Stmt::For { span, .. } => return refuse("a `for` loop", *span),
            Stmt::Break(span) => return refuse("`break`", *span),
            Stmt::Continue(span) => return refuse("`continue`", *span),
        })
    }
}

/// How many slots `pattern` binds: one for each `Bind`.
fn binds(pattern: &Pattern) -> usize {
    match pattern {
        Pattern::Bind(_) => 1,
        Pattern::Wildcard | Pattern::Literal(_) => 0,
        Pattern::Construct { fields, .. } => fields.iter().map(binds).sum(),
    }
}

/// A JSON value, written by hand so the crate stays dependency-free.
/// Object fields keep their insertion order.
#[derive(Debug, Clone)]
enum Json {
    Bool(bool),
    Int(i128),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

/// A value whose compact form fits in this many columns prints on one
/// line.
const LINE_WIDTH: usize = 80;

impl Json {
    fn compact(&self, out: &mut String) {
        match self {
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Str(s) => {
                out.push('"');
                out.push_str(&json_escape(s));
                out.push('"');
            }
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.compact(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('"');
                    out.push_str(key);
                    out.push_str("\": ");
                    value.compact(out);
                }
                out.push('}');
            }
        }
    }

    /// Print with two-space indentation from column `indent`, keeping a
    /// value that fits on one line.
    fn pretty(&self, out: &mut String, indent: usize) {
        let mut flat = String::new();
        self.compact(&mut flat);
        if indent + flat.len() <= LINE_WIDTH {
            out.push_str(&flat);
            return;
        }
        let pad = " ".repeat(indent + 2);
        let (open, close, items): (char, char, Vec<(Option<&str>, &Json)>) = match self {
            Json::Arr(items) => ('[', ']', items.iter().map(|v| (None, v)).collect()),
            Json::Obj(fields) => (
                '{',
                '}',
                fields.iter().map(|(k, v)| (Some(*k), v)).collect(),
            ),
            Json::Bool(_) | Json::Int(_) | Json::Str(_) => {
                out.push_str(&flat);
                return;
            }
        };
        out.push(open);
        out.push('\n');
        for (i, (key, value)) in items.iter().enumerate() {
            out.push_str(&pad);
            if let Some(key) = key {
                out.push('"');
                out.push_str(key);
                out.push_str("\": ");
            }
            value.pretty(out, pad.len());
            if i + 1 < items.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&pad[2..]);
        out.push(close);
    }
}
