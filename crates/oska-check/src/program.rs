//! The **checked program**: what the checker hands to every backend
//! (ADR 0007).
//!
//! It is a lowered tree in which every name and call is already resolved:
//! a local is a slot, a call names the function or intrinsic it reaches.
//! A backend never re-resolves names or dispatch; when it needs something
//! the tree does not say, the tree gains it. The shape is internal to the
//! toolchain and may change freely, unlike the AST JSON export.
//!
//! [`crate::check_module`] lowers a program only when it checks with no
//! diagnostics (`program/lower.rs`), so a backend only ever sees programs
//! that passed the checker.
//!
//! # Adding a node kind
//!
//! 1. Add a variant to [`ExprKind`] (or [`Stmt`]).
//! 2. If the node needs something the checker decides, such as a call's
//!    target, record it while checking: `Body::record(span,
//!    Resolution::...)` in `body/`, adding a `lower::Resolution` variant
//!    when none fits.
//! 3. Lower the AST node in `lower::Lowering::expr`, which turns anything
//!    it does not know yet into [`ExprKind::Unsupported`].
//! 4. Evaluate it in each backend.

use std::collections::BTreeMap;

use oska_syntax::Span;

pub(crate) mod lower;

/// A checked program: its functions, keyed by qualified name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    /// Every function with a body, keyed by qualified name: `name` for a
    /// free function.
    pub functions: BTreeMap<String, Function>,
}

/// A function with a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The qualified name, as [`Program::functions`] keys it.
    pub name: String,
    /// The parameters, in order.
    pub params: Vec<Param>,
    pub body: Expr,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// The declared type, as the checker prints it, such as `Stdout`.
    pub ty: String,
    /// Whether the parameter takes a local slot. A parameter named `_`
    /// takes none: its argument is evaluated and dropped.
    pub binds: bool,
}

/// An expression, with the span of the source it was lowered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

/// What an expression does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    /// `()`.
    Unit,
    /// A string literal without interpolation.
    Str(String),
    /// A local, by slot. Slots are numbered from 0 in each function's
    /// frame, in the order locals are bound: the binding parameters first.
    Local(usize),
    /// A call. A method's receiver is its first argument.
    Call { callee: Callee, args: Vec<Expr> },
    /// A block: its statements in order, then its value.
    Block {
        stmts: Vec<Stmt>,
        value: Option<Box<Expr>>,
    },
    /// Something the checked program cannot express yet. A backend
    /// reports it when it is reached.
    Unsupported,
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// An expression evaluated for its effect.
    Expr(Expr),
    /// `let` or `var`: evaluate `value` and, when `binds`, push it as the
    /// next slot. A binding named `_` takes no slot.
    Let { binds: bool, value: Expr },
}

/// What a call reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    /// A function in [`Program::functions`], by qualified name.
    Function(String),
    /// A body-less prelude function, which each backend implements, by
    /// qualified name: `Type.method` for a method, such as
    /// `Stdout.println`.
    Intrinsic(String),
}
