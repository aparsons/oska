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
//! # Version 4
//!
//! Oska Core version 4 has functions, `let`, `if`, `Int` and `Bool` with
//! their operators, `()`, the value of a block or `if` with no value, and
//! the module's own structs and enums without generics, with their
//! construction, field access and `match`. It has text (`String`, trusted
//! or not, as literals), `Path`, and the capabilities `Stdout`, `Fs`,
//! `FsRead`, `FsWrite`, `Dir`, `DirRead` and `DirWrite`, as opaque values
//! that only `main` receives, with the built-in calls in [`BUILTINS`]:
//! writing a line to stdout, reading and writing a file and creating a
//! directory under a directory capability, attenuation, and `is_ok` and
//! `unwrap_or` on the `Result` an effect gives. It has closures, which
//! capture by value the slots they read, function types, and calls of a
//! function value. [`export`] refuses any other
//! program, naming the first construct outside it in source order, with
//! its span. It fails closed: a trait, `impl` or `const` is refused even
//! when nothing uses it, and so are a generic struct or enum, one with a
//! field whose type is outside Oska Core (refused at its declaration),
//! `var` (even one never assigned), a `use`, a `test` block, a generic
//! function, a method, a contract, a prelude type such as `Option`, and
//! any type the checker left unsolved. It also refuses any other built-in
//! call, `==` and `!=` on anything but `Int`, `Bool` and `()`, a struct or
//! enum with a capability or function field, a closure that reads `self`, a
//! function used as a value, and a `main` that returns anything but
//! `()` or takes anything but `Stdout`, `Fs`, `FsRead` or `FsWrite`, the
//! capabilities the runtime grants.
//!
//! The text is pretty-printed JSON: two-space indentation, a value that
//! fits in 80 columns on one line, and a final newline. Every node is an
//! object whose first field is `kind`; a field shown with `?` is left out
//! when absent, never written as `null`.
//!
//! ```text
//! program  = {"core_ir_version": 4, "types": [typedecl...],
//!             "functions": [function...]}
//! typedecl = {"kind": "Struct", "name": NAME, "fields": [type...]}
//!          | {"kind": "Enum", "name": NAME, "variants": [variant...]}
//! variant  = {"name": NAME, "fields": [type...]}
//! function = {"name": NAME, "params": [param...], "ret": type, "body": expr}
//! param    = {"type": type, "binds": BOOL}
//! type     = "Int" | "Bool" | "()" | "String" | "Path" | CAPABILITY
//!          | "Result<(), IoError>" | "Result<String, IoError>" | NAME
//!          | {"kind": "Fn", "params": [type...], "ret": type}
//! expr     = {"kind": "Unit"}
//!          | {"kind": "Int", "value": INTEGER}
//!          | {"kind": "Bool", "value": BOOL}
//!          | {"kind": "Str", "value": STRING}
//!          | {"kind": "Local", "slot": N}
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
//!             "height": N, "ret": type, "body": expr}
//!          | {"kind": "CallValue", "callee": expr, "args": [expr...]}
//! stmt     = {"kind": "Let", "binds": BOOL, "type": type, "value": expr}
//!          | {"kind": "Expr", "value": expr}
//! arm      = {"pattern": pattern, "guard"?: expr, "body": expr}
//! pattern  = {"kind": "Wildcard"}
//!          | {"kind": "Bind", "type": type}
//!          | {"kind": "Unit"}
//!          | {"kind": "Int", "value": INTEGER}
//!          | {"kind": "Bool", "value": BOOL}
//!          | {"kind": "Construct", "type": NAME, "variant"?: NAME,
//!             "fields": [pattern...]}
//! ```
//!
//! - `CAPABILITY` is `"Stdout"`, `"Fs"`, `"FsRead"`, `"FsWrite"`,
//!   `"Dir"`, `"DirRead"` or `"DirWrite"`, and `BUILTIN` one of
//!   [`BUILTINS`], the checker's name for a body-less prelude function. A
//!   method's receiver is its first argument. No field of a struct or
//!   enum is a capability or a function.
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
//!   dropped.
//! - A `Closure` is written where `height` slots are in scope. Its body
//!   numbers slots as the enclosing function does there, then its
//!   parameters from `height` on, then its own locals. `captures` lists,
//!   in ascending order, the slots below `height` its body reads, a
//!   closure inside it included. Evaluating it makes a function value of
//!   type `fn(params) -> ret` that holds the values of those slots, by
//!   value. `CallValue` evaluates `callee`, then `args` left to right, then
//!   runs the closure's body with what it captured at their slots and the
//!   arguments at its parameters'. A `Fn` type does not say whether it is
//!   a `cap fn`: what a function value reaches is what it captured.
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
//! - `main` takes only capabilities the runtime grants: `Stdout`, and
//!   `Fs`, `FsRead` or `FsWrite`, which reach anywhere until attenuated.
//!
//! # Adding a construct
//!
//! A construct joins Oska Core in a new version: bump [`CORE_IR_VERSION`],
//! extend the grammar above and [`export`], refresh the goldens with
//! `UPDATE_GOLDEN=1`, and update the Lean model in the same pull request
//! (ADR 0018).

use std::collections::BTreeSet;
use std::fmt;

use oska_syntax::{json_escape, Span};

use crate::program::{
    Arm, Callee, Construct, Declaration, DeclarationKind, Expr, ExprKind, Function, IntType, Param,
    Pattern, Program, Stmt, Type, TypeDeclKind,
};

/// The version of the core IR [`export`] writes.
pub const CORE_IR_VERSION: u64 = 4;

/// The built-in calls of Oska Core, by the checker's qualified name: the
/// effects on a capability, attenuation, `Path` and the `Result` of an
/// effect.
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
];

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
            Ok(json) => types.push(json),
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
/// module's structs and enums that are in Oska Core.
struct Core {
    types: BTreeSet<String>,
}

impl Core {
    /// The core IR of a struct or enum declaration, or the refusal of a
    /// declaration outside Oska Core.
    fn declaration(&self, program: &Program, d: &Declaration) -> Result<Json, Refusal> {
        let kind = match d.kind {
            DeclarationKind::Struct => "struct",
            DeclarationKind::Enum => "enum",
            DeclarationKind::Trait => return refuse(format!("the trait `{}`", d.name), d.span),
            DeclarationKind::Impl => return refuse(format!("the `impl` of `{}`", d.name), d.span),
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
        Ok(match &decl.kind {
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
        })
    }

    fn function(&self, f: &Function) -> Result<Json, Refusal> {
        if f.self_ty.is_some() {
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
        // In source order: the parameters, the result type, the contracts.
        let params = self.params(&f.params, f.span)?;
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
        Ok(Json::Obj(vec![
            ("name", Json::Str(f.name.clone())),
            ("params", params),
            ("ret", ret),
            ("body", self.expr(&f.body)?),
        ]))
    }

    /// The parameters of a function or closure, each refused at its
    /// annotation, or at `span` when it has none.
    fn params(&self, params: &[Param], span: Span) -> Result<Json, Refusal> {
        Ok(Json::Arr(
            params
                .iter()
                .map(|p| {
                    Ok(Json::Obj(vec![
                        ("type", self.ty(&p.ty, p.annotation.unwrap_or(span))?),
                        ("binds", Json::Bool(p.binds)),
                    ]))
                })
                .collect::<Result<_, Refusal>>()?,
        ))
    }

    /// A type of Oska Core, or the refusal of another, at `span`.
    fn ty(&self, ty: &Type, span: Span) -> Result<Json, Refusal> {
        if let Type::Fn { params, ret } = ty {
            return Ok(node(
                "Fn",
                vec![
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
            Type::Named { name, args } if name == "Result" => match args.as_slice() {
                [Type::Unit, Type::Named {
                    name: e,
                    args: none,
                }] if e == "IoError" && none.is_empty() => "Result<(), IoError>",
                [Type::String, Type::Named {
                    name: e,
                    args: none,
                }] if e == "IoError" && none.is_empty() => "Result<String, IoError>",
                _ => return refuse(format!("the type `{ty}`"), span),
            },
            Type::Int(_) | Type::Named { .. } | Type::Fn { .. } | Type::Param(_) | Type::Never => {
                return refuse(format!("the type `{ty}`"), span)
            }
        };
        Ok(Json::Str(name.to_string()))
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
            ExprKind::Bool(value) => node("Bool", vec![("value", Json::Bool(*value))]),
            ExprKind::Local(slot) => node("Local", vec![("slot", Json::Int(*slot as i128))]),
            ExprKind::Neg { int, .. } | ExprKind::Binary { int: Some(int), .. }
                if *int != IntType::Int =>
            {
                return refuse(format!("the integer type `{}`", int.name()), span)
            }
            ExprKind::Neg { operand, .. } => node("Neg", vec![("operand", self.expr(operand)?)]),
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
            } => match callee {
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
                Callee::Intrinsic(name) if BUILTINS.contains(&name.as_str()) => node(
                    "Builtin",
                    vec![
                        ("name", Json::Str(name.clone())),
                        ("args", self.exprs(args)?),
                    ],
                ),
                Callee::Intrinsic(name) => return refuse(format!("the built-in `{name}`"), span),
                Callee::Trait { trait_name, method } => {
                    return refuse(format!("the trait method `{trait_name}.{method}`"), span)
                }
            },
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut fields = vec![
                    ("cond", self.expr(cond)?),
                    ("then", self.expr(then_branch)?),
                ];
                if let Some(else_branch) = else_branch {
                    fields.push(("else", self.expr(else_branch)?));
                }
                node("If", fields)
            }
            ExprKind::Block { stmts, value } => {
                let stmts = stmts
                    .iter()
                    .map(|s| self.stmt(s))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut fields = vec![("stmts", Json::Arr(stmts))];
                if let Some(value) = value {
                    fields.push(("value", self.expr(value)?));
                }
                node("Block", fields)
            }
            ExprKind::Str(value) => node("Str", vec![("value", Json::Str(value.clone()))]),
            ExprKind::Interpolated(_) => return refuse("an interpolated string", span),
            ExprKind::SelfValue => return refuse("`self`", span),
            ExprKind::List(_) => return refuse("a list", span),
            ExprKind::Construct { ctor, fields } => {
                let mut parts = self.ctor(ctor, span)?;
                parts.push(("fields", self.exprs(fields)?));
                node("Construct", parts)
            }
            ExprKind::Field { base, index } => {
                let json = self.expr(base)?;
                // Only a struct of Oska Core has fields to read.
                self.ty(&base.ty, base.span)?;
                node(
                    "Field",
                    vec![("base", json), ("index", Json::Int(*index as i128))],
                )
            }
            ExprKind::Match { scrutinee, arms } => {
                let json = self.expr(scrutinee)?;
                self.ty(&scrutinee.ty, scrutinee.span)?;
                let arms = arms
                    .iter()
                    .map(|arm| self.arm(arm, span))
                    .collect::<Result<Vec<_>, _>>()?;
                node(
                    "Match",
                    vec![("scrutinee", json), ("arms", Json::Arr(arms))],
                )
            }
            ExprKind::Closure(closure) => {
                if closure.captures_self {
                    return refuse("a closure that reads `self`", span);
                }
                // The closure's type is not written: its result type is
                // refused at the closure, before its parameters and body.
                let Type::Fn { ret, .. } = &e.ty else {
                    return refuse(format!("a closure of type `{}`", e.ty), span);
                };
                let ret = self.ty(ret, span)?;
                let params = self.params(&closure.params, span)?;
                node(
                    "Closure",
                    vec![
                        ("params", params),
                        (
                            "captures",
                            Json::Arr(
                                closure
                                    .captures
                                    .iter()
                                    .map(|&slot| Json::Int(slot as i128))
                                    .collect(),
                            ),
                        ),
                        ("height", Json::Int(closure.height as i128)),
                        ("ret", ret),
                        ("body", self.expr(&closure.body)?),
                    ],
                )
            }
            ExprKind::CallValue { callee, args } => node(
                "CallValue",
                vec![("callee", self.expr(callee)?), ("args", self.exprs(args)?)],
            ),
            ExprKind::MutatingCall { .. } => return refuse("a mutating method call", span),
            ExprKind::Range { .. } => return refuse("a range", span),
            ExprKind::Try { .. } => return refuse("`?`", span),
            ExprKind::FnValue { .. } => return refuse("a function used as a value", span),
            ExprKind::Scope(_) => return refuse("`scope`", span),
            ExprKind::Unsupported(what) => return refuse(what.clone(), span),
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

    /// A `match` arm of Oska Core, or the refusal of the first construct in
    /// it that is not. A pattern has no span of its own: one outside Oska
    /// Core is refused at the `match`, `span`.
    fn arm(&self, arm: &Arm, span: Span) -> Result<Json, Refusal> {
        let mut fields = vec![("pattern", self.pattern(&arm.pattern, span)?)];
        if let Some(guard) = &arm.guard {
            fields.push(("guard", self.expr(guard)?));
        }
        fields.push(("body", self.expr(&arm.body)?));
        Ok(Json::Obj(fields))
    }

    /// A pattern of Oska Core, or the refusal of one that is not.
    fn pattern(&self, pattern: &Pattern, span: Span) -> Result<Json, Refusal> {
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
            Pattern::Construct { ctor, fields } => {
                let mut parts = self.ctor(ctor, span)?;
                let fields = fields
                    .iter()
                    .map(|p| self.pattern(p, span))
                    .collect::<Result<Vec<_>, _>>()?;
                parts.push(("fields", Json::Arr(fields)));
                node("Construct", parts)
            }
        })
    }

    /// A statement of Oska Core, or the refusal of the first construct in
    /// it that is not.
    fn stmt(&self, s: &Stmt) -> Result<Json, Refusal> {
        Ok(match s {
            Stmt::Expr(value) => node("Expr", vec![("value", self.expr(value)?)]),
            Stmt::Let {
                mutable: true,
                span,
                ..
            } => return refuse("a `var` binding", *span),
            Stmt::Let {
                binds,
                mutable: false,
                ty,
                annotation,
                value,
                ..
            } => {
                // A written type comes before the value; an inferred one is
                // the value's.
                let (ty, value) = match annotation {
                    Some(annotation) => {
                        let ty = self.ty(ty, *annotation)?;
                        (ty, self.expr(value)?)
                    }
                    None => {
                        let json = self.expr(value)?;
                        (self.ty(ty, value.span)?, json)
                    }
                };
                node(
                    "Let",
                    vec![
                        ("binds", Json::Bool(*binds)),
                        ("type", ty),
                        ("value", value),
                    ],
                )
            }
            Stmt::Assign { span, .. } => return refuse("an assignment", *span),
            Stmt::Return { span, .. } => return refuse("`return`", *span),
            Stmt::While { span, .. } => return refuse("a `while` loop", *span),
            Stmt::For { span, .. } => return refuse("a `for` loop", *span),
            Stmt::Break(span) => return refuse("`break`", *span),
            Stmt::Continue(span) => return refuse("`continue`", *span),
        })
    }
}

/// A JSON value, written by hand so the crate stays dependency-free.
/// Object fields keep their insertion order.
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
