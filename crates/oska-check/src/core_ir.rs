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
//! # Version 1
//!
//! Oska Core version 1 has functions, `let`, `if`, `Int` and `Bool` with
//! their operators, and `()`, the value of a block or `if` with no value.
//! [`export`] refuses any other program, naming the first construct
//! outside it in source order, with its span.
//!
//! The text is pretty-printed JSON: two-space indentation, a value that
//! fits in 80 columns on one line, and a final newline. Every node is an
//! object whose first field is `kind`; a field shown with `?` is left out
//! when absent, never written as `null`.
//!
//! ```text
//! program  = {"core_ir_version": 1, "functions": [function...]}
//! function = {"name": NAME, "params": [param...], "ret": type, "body": expr}
//! param    = {"type": type, "binds": BOOL}
//! type     = "Int" | "Bool" | "()"
//! expr     = {"kind": "Unit"}
//!          | {"kind": "Int", "value": INTEGER}
//!          | {"kind": "Bool", "value": BOOL}
//!          | {"kind": "Local", "slot": N}
//!          | {"kind": "Neg", "operand": expr}
//!          | {"kind": "Not", "operand": expr}
//!          | {"kind": "Binary", "op": OP, "lhs": expr, "rhs": expr}
//!          | {"kind": "Call", "function": NAME, "args": [expr...]}
//!          | {"kind": "If", "cond": expr, "then": expr, "else"?: expr}
//!          | {"kind": "Block", "stmts": [stmt...], "value"?: expr}
//! stmt     = {"kind": "Let", "binds": BOOL, "type": type, "value": expr}
//!          | {"kind": "Expr", "value": expr}
//! ```
//!
//! - `functions` are in source order. `NAME` is a function's name, which
//!   a `Call` uses to reach it. A constant is a function with no
//!   parameters, as the checked program has it.
//! - Slots number a function's locals from 0, in the order they are
//!   bound: the parameters that bind, then each `let` that binds, as it
//!   runs. A `let` is in scope until the end of its block. A parameter
//!   or `let` named `_` has `"binds": false` and takes no slot: its value
//!   is evaluated and dropped.
//! - `Int` is a signed 64-bit integer. A literal is written in decimal,
//!   negative when the source writes `-` before it. `Neg` and the
//!   arithmetic operators panic when the result does not fit, and `/`
//!   and `%` when the divisor is 0: `/` rounds toward zero, and `%` has
//!   the sign of its left operand.
//! - `OP` is the operator as written: `+ - * / %` on `Int`; `< <= > >=`
//!   on `Int`, giving `Bool`; `== !=` on two values of one type; and
//!   `&& || implies` on `Bool`, which evaluate `rhs` only when the result
//!   needs it.
//! - `If` evaluates `then` when `cond` is true and `else` when it is
//!   false. Without `else` it has the value `()`, and drops the value of
//!   `then`. A `Block` evaluates its statements in order, dropping the
//!   value of each `Expr`, then its value, or `()` without one.
//! - Arguments are evaluated left to right before the call.
//!
//! # Adding a construct
//!
//! A construct joins Oska Core in a new version: bump [`CORE_IR_VERSION`],
//! extend the grammar above and [`export`], refresh the goldens with
//! `UPDATE_GOLDEN=1`, and update the Lean model in the same pull request
//! (ADR 0018).

use std::fmt;

use oska_syntax::{json_escape, Span};

use crate::program::{Callee, Expr, ExprKind, Function, IntType, Param, Program, Stmt, Type};

/// The version of the core IR [`export`] writes.
pub const CORE_IR_VERSION: u64 = 1;

/// Why a program has no core IR: its first construct outside Oska Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The construct, such as "the struct `Point`" or "a `while` loop".
    pub construct: String,
    /// Where it is in the source. A construct of a function's signature,
    /// such as a parameter's type, is located at the function's name; a
    /// statement with no span of its own at its first expression.
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
    let mut functions: Vec<&Function> = program.functions.values().collect();
    functions.sort_by_key(|f| f.span.start);
    let mut refusals: Vec<Refusal> = Vec::new();
    let mut exported = Vec::new();
    for function in functions {
        match self::function(function) {
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

fn function(f: &Function) -> Result<Json, Refusal> {
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
    if let Some(clause) = f
        .contracts
        .requires
        .iter()
        .chain(f.contracts.ensures.iter().map(|e| &e.clause))
        .min_by_key(|c| c.span.start)
    {
        return refuse("a contract", clause.span);
    }
    let params = f
        .params
        .iter()
        .map(|p: &Param| {
            Ok(Json::Obj(vec![
                ("type", ty(&p.ty, f.span)?),
                ("binds", Json::Bool(p.binds)),
            ]))
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let ret = ty(&f.ret, f.span)?;
    Ok(Json::Obj(vec![
        ("name", Json::Str(f.name.clone())),
        ("params", Json::Arr(params)),
        ("ret", ret),
        ("body", expr(&f.body)?),
    ]))
}

/// A type of Oska Core, or the refusal of another, at `span`.
fn ty(ty: &Type, span: Span) -> Result<Json, Refusal> {
    let name = match ty {
        Type::Unit => "()",
        Type::Bool => "Bool",
        Type::Int(IntType::Int) => "Int",
        Type::Int(_)
        | Type::String
        | Type::Named { .. }
        | Type::Fn { .. }
        | Type::Param(_)
        | Type::Never => return refuse(format!("the type `{ty}`"), span),
    };
    Ok(Json::Str(name.to_string()))
}

fn node(kind: &'static str, mut fields: Vec<(&'static str, Json)>) -> Json {
    fields.insert(0, ("kind", Json::Str(kind.to_string())));
    Json::Obj(fields)
}

fn exprs(items: &[Expr]) -> Result<Json, Refusal> {
    Ok(Json::Arr(items.iter().map(expr).collect::<Result<_, _>>()?))
}

/// An expression of Oska Core, or the refusal of the first construct in
/// it that is not. A node is refused before anything inside it, and the
/// parts of a node in source order, so the first refusal met is the
/// first in the source.
fn expr(e: &Expr) -> Result<Json, Refusal> {
    let span = e.span;
    Ok(match &e.kind {
        ExprKind::Unit => node("Unit", Vec::new()),
        ExprKind::Int(value, IntType::Int) => node("Int", vec![("value", Json::Int(*value))]),
        ExprKind::Int(_, int) => return refuse(format!("the integer type `{}`", int.name()), span),
        ExprKind::Bool(value) => node("Bool", vec![("value", Json::Bool(*value))]),
        ExprKind::Local(slot) => node("Local", vec![("slot", Json::Int(*slot as i128))]),
        ExprKind::Neg { operand, .. } => node("Neg", vec![("operand", expr(operand)?)]),
        ExprKind::Not(operand) => node("Not", vec![("operand", expr(operand)?)]),
        ExprKind::Binary { op, lhs, rhs, .. } => node(
            "Binary",
            vec![
                ("op", Json::Str(op.text().to_string())),
                ("lhs", expr(lhs)?),
                ("rhs", expr(rhs)?),
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
                    ("args", exprs(args)?),
                ],
            ),
            Callee::Function(name) => {
                return refuse(format!("a call of the generic function `{name}`"), span)
            }
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
            let mut fields = vec![("cond", expr(cond)?), ("then", expr(then_branch)?)];
            if let Some(else_branch) = else_branch {
                fields.push(("else", expr(else_branch)?));
            }
            node("If", fields)
        }
        ExprKind::Block { stmts, value } => {
            let stmts = stmts
                .iter()
                .map(|s| stmt(s, span))
                .collect::<Result<Vec<_>, _>>()?;
            let mut fields = vec![("stmts", Json::Arr(stmts))];
            if let Some(value) = value {
                fields.push(("value", expr(value)?));
            }
            node("Block", fields)
        }
        ExprKind::Str(_) | ExprKind::Interpolated(_) => return refuse("a string", span),
        ExprKind::SelfValue => return refuse("`self`", span),
        ExprKind::List(_) => return refuse("a list", span),
        ExprKind::Construct { ctor, .. } => {
            let construct = match &ctor.variant {
                Some(variant) => format!("the enum variant `{}.{variant}`", ctor.ty),
                None => format!("the struct `{}`", ctor.ty),
            };
            return refuse(construct, span);
        }
        ExprKind::Field { .. } => return refuse("a field access", span),
        ExprKind::Closure(_) => return refuse("a closure", span),
        ExprKind::CallValue { .. } => return refuse("a call of a function value", span),
        ExprKind::MutatingCall { .. } => return refuse("a mutating method call", span),
        ExprKind::Range { .. } => return refuse("a range", span),
        ExprKind::Try { .. } => return refuse("`?`", span),
        ExprKind::FnValue { .. } => return refuse("a function used as a value", span),
        ExprKind::Match { .. } => return refuse("`match`", span),
        ExprKind::Scope(_) => return refuse("`scope`", span),
        ExprKind::Unsupported(what) => return refuse(what.clone(), span),
    })
}

/// A statement of Oska Core in the block at `block`, or the refusal of
/// the first construct in it that is not. A statement with no span of
/// its own is located at its first expression.
fn stmt(s: &Stmt, block: Span) -> Result<Json, Refusal> {
    Ok(match s {
        Stmt::Expr(value) => node("Expr", vec![("value", expr(value)?)]),
        Stmt::Let { binds, ty, value } => {
            let value_json = expr(value)?;
            node(
                "Let",
                vec![
                    ("binds", Json::Bool(*binds)),
                    ("type", self::ty(ty, value.span)?),
                    ("value", value_json),
                ],
            )
        }
        Stmt::Assign { value, .. } => return refuse("an assignment", value.span),
        Stmt::Return { span, .. } => return refuse("`return`", *span),
        Stmt::While { cond, .. } => return refuse("a `while` loop", cond.span),
        Stmt::For { iter, .. } => return refuse("a `for` loop", iter.span),
        // Only inside a loop, which is refused first.
        Stmt::Break => return refuse("`break`", block),
        Stmt::Continue => return refuse("`continue`", block),
    })
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
