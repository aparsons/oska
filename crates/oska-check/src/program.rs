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
use std::rc::Rc;

use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

pub(crate) mod lower;

/// A checked program: its functions, keyed by qualified name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    /// Every function with a body, keyed by qualified name: `name` for a
    /// free function, `Type.method` for a method or associated function
    /// of an inherent impl, and `<Type as Trait>.method` for a method of
    /// a trait impl (see [`trait_method`]).
    pub functions: BTreeMap<String, Function>,
}

/// The qualified name of `method` in the impl of `trait_name` for
/// `type_name`, such as `<Point as Display>.to_string`.
pub fn trait_method(type_name: &str, trait_name: &str, method: &str) -> String {
    format!("<{type_name} as {trait_name}>.{method}")
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
    /// A list literal, `[a, b, c]`.
    List(Vec<Expr>),
    /// A struct or enum variant built from its fields, in declaration
    /// order. A fieldless variant has none.
    Construct {
        ctor: Construct,
        fields: Vec<Expr>,
    },
    /// Field `index` of a struct, counted in declaration order.
    Field {
        base: Box<Expr>,
        index: usize,
    },
    /// A closure, `|params| body`. Evaluating it captures the frame's
    /// slots and receiver as they are, by value.
    Closure(Rc<Closure>),
    /// A call of a function value, such as a closure held in a local.
    CallValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// A call. A method's receiver is its first argument.
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
    /// A call of a **mutating method** (`var self`) on a place, such as
    /// `xs.push(x)` or `self.items.push(x)`. The arguments are evaluated
    /// first; then the receiver is taken out of `receiver`, so that a
    /// value nothing else shares is changed in place, and the method's
    /// final `self` is written back to it.
    MutatingCall {
        callee: Callee,
        receiver: Place,
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
    /// `match`: the first arm whose pattern matches the scrutinee, and
    /// whose guard then holds, gives the value. The checker guarantees
    /// one does.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
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
    /// `for binding in iter { body }`, over a range or a `List`: each
    /// element is pushed as the next slot when `binds`, for the body to
    /// see.
    For {
        binds: bool,
        iter: Expr,
        body: Expr,
    },
}

/// A closure's code.
///
/// Its body sees the slots in scope where the closure was written, by
/// the same numbers, then its parameters: so a closure value carries the
/// slots it captured, and a call pushes each argument after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closure {
    /// Whether each parameter takes a slot: one named `_` takes none.
    pub params: Vec<bool>,
    pub body: Expr,
}

/// A `match` arm. The pattern's bindings are pushed as slots, in
/// source order, for the guard and the body to see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

/// A pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `_`: matches anything, and binds nothing.
    Wildcard,
    /// A name: matches anything, and pushes it as the next slot.
    Bind,
    /// A literal (an integer, string, `Bool`, or `()`): matches an equal
    /// value.
    Literal(Expr),
    /// A struct, which always matches its type, or an enum variant: then
    /// each field, in declaration order, matches its pattern.
    Construct {
        ctor: Construct,
        fields: Vec<Pattern>,
    },
}

/// Something that can be assigned to, or that a mutating method
/// changes. Writing inside a value that is shared copies it first, so
/// changing a copy never changes the original (ADR 0001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A local, by slot.
    Local(usize),
    /// `self`, in a mutating method.
    SelfValue,
    /// Field `index` of the struct at `base`.
    Field { base: Box<Place>, index: usize },
}

/// A part of an interpolated string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// A hole: its value, turned to text by `display`, the `Display`
    /// implementation the checker chose: `Int.to_string`, or a user
    /// type's `<Point as Display>.to_string`.
    Hole {
        value: Expr,
        display: Callee,
    },
}

/// What a constructor builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Construct {
    /// The struct or enum's name, such as `Point` or `Option`.
    pub ty: String,
    /// The variant, for an enum, such as `Some`.
    pub variant: Option<String>,
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
