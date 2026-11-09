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
//!    when none fits. When it depends on a type inference may not have
//!    solved yet, such as an operation's integer type, use
//!    `Body::defer(span, ty, Deferred::...)`, which records it once the
//!    body is checked.
//! 3. Lower the AST node in `lower::Lowering::expr` (or `stmt`), which
//!    turns anything it does not know yet into [`ExprKind::Unsupported`].
//! 4. Evaluate it in each backend: in `oska-run`, `interp.rs`, and let
//!    `support.rs` walk into it, since it refuses (`E0380`) a program with
//!    an `Unsupported` node or a call it cannot make.

use std::collections::BTreeMap;

use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

pub(crate) mod lower;

/// A checked program: its functions, keyed by qualified name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    /// Every function with a body, keyed by qualified name: `name` for a
    /// free function, `Type.method` for a method or associated function
    /// of an inherent impl. (Trait impl methods are not lowered yet.)
    pub functions: BTreeMap<String, Function>,
    /// The module each `use` imports, such as `std.sql`, with the span of
    /// its path, in source order.
    pub imports: Vec<(String, Span)>,
}

/// A function with a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The qualified name, as [`Program::functions`] keys it.
    pub name: String,
    /// A method's receiver, `self` or `var self`: the caller passes it
    /// before the parameters, and it takes no slot ([`ExprKind::SelfValue`]
    /// reads it).
    pub receiver: bool,
    /// The parameters after the receiver, in order.
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
    /// An interpolated string: its parts, concatenated.
    Interpolated(Vec<Part>),
    /// An integer literal, negated when written with a leading `-`.
    Int(i128),
    Bool(bool),
    /// A binary operator. `int` is the operands' integer type for
    /// arithmetic (`+ - * / %`), whose result is checked against it, and
    /// `None` otherwise. `&&`, `||`, and `implies` evaluate `rhs` only
    /// when they need it. `==` and `!=` compare values of built-in types.
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        int: Option<IntType>,
    },
    /// Unary `-` on an integer of type `int`.
    Neg {
        operand: Box<Expr>,
        int: IntType,
    },
    /// `!`.
    Not(Box<Expr>),
    /// A local, by slot. Slots are numbered from 0 in each function's
    /// frame, in the order locals are bound: the binding parameters first.
    Local(usize),
    /// `self`, the receiver of the method being run.
    SelfValue,
    /// A call. A method's receiver is its first argument.
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
    /// A block: its statements in order, then its value.
    Block {
        stmts: Vec<Stmt>,
        value: Option<Box<Expr>>,
    },
    /// `start..end`: the integers from `start`, up to but excluding
    /// `end`.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    /// `if`, with `else` or without. The branches are blocks, or another
    /// `if` for `else if`.
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
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
    Let {
        binds: bool,
        value: Expr,
    },
    /// `place = value`.
    Assign {
        place: Place,
        value: Expr,
    },
    /// `return`, with `()` when no value is written.
    Return(Option<Expr>),
    Break,
    Continue,
    /// `while cond { body }`.
    While {
        cond: Expr,
        body: Expr,
    },
    /// `for binding in iter { body }`: each element is pushed as the next
    /// slot when `binds`, for the body to see.
    For {
        binds: bool,
        iter: Expr,
        body: Expr,
    },
}

/// Something that can be assigned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A local, by slot.
    Local(usize),
}

/// A part of an interpolated string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// A hole: its value, turned to text by `display`, the `Display`
    /// implementation the checker chose (`Int.to_string`).
    Hole {
        value: Expr,
        display: Callee,
    },
}

/// An integer type, with its inclusive range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntType {
    /// Its name, such as `Int` or `U8`.
    pub name: String,
    pub min: i128,
    pub max: i128,
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
