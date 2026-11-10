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
use std::sync::Arc;

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
    /// The module each `use` imports, such as `std.sql`, with the span of
    /// its path, in source order.
    pub imports: Vec<(String, Span)>,
    /// Every `test` block, in source order.
    pub tests: Vec<Test>,
}

/// A `test` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    /// The name as written, without its quotes.
    pub name: String,
    /// Marked `#[integration]`: it runs only on request, with real
    /// capabilities.
    pub integration: bool,
    /// The body. Slot 0 holds `t`, the `TestContext`; its value, if any,
    /// is dropped.
    pub body: Expr,
    /// The whole item, its attributes included.
    pub span: Span,
}

/// The qualified name of `method` in the impl of `trait_name` for
/// `type_name`, such as `<Point as Display>.to_string`. For a generic
/// trait, `trait_name` carries the impl's arguments, as `From<IoError>`
/// does in `<AppError as From<IoError>>.from`, so that two impls of one
/// trait for one type have different names (see
/// [`crate::items::ImplInfo::trait_ref`]).
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
    /// An integer literal, negated when written with a leading `-`, of
    /// the integer type the checker gave it.
    Int(i128, IntType),
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
    /// A closure, `|params| body`. Evaluating it captures, by value, the
    /// slots the body reads from the enclosing frame, and the receiver
    /// when it reads `self`.
    Closure(Arc<Closure>),
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
    /// `operand?`: the success value of a `Result` (`Ok`) or an `Option`
    /// (`Some`). On `None` the function returns `None`. On `Err(e)` it
    /// returns `Err(e)`, or `Err(convert(e))` when the checker chose a
    /// `From` implementation to convert the error with, such as
    /// `<AppError as From<IoError>>.from`.
    Try {
        operand: Box<Expr>,
        convert: Option<Callee>,
    },
    /// A function used as a value, such as `double` in `let f = double`.
    /// Calling the value calls the function.
    FnValue(Callee),
    /// `match`: the first arm whose pattern matches the scrutinee, and
    /// whose guard then holds, gives the value. The checker guarantees
    /// one does.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
    },
    /// `scope |s| { ... }`. The handle `s` takes the next slot, for the
    /// body to see. The body is a block.
    Scope(Box<Expr>),
    /// Something the checked program cannot express yet, named as a
    /// refusal names it, such as "a `From`-bound conversion". A backend
    /// refuses a program that has one.
    Unsupported(String),
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
/// Its body numbers slots as the enclosing frame does where the closure
/// was written, then its parameters from [`Closure::height`] on. A
/// closure value carries only the slots in [`Closure::captures`]; a call
/// puts each back at its number, below `height`, and pushes each argument
/// after them. The other slots below `height` are never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closure {
    /// Whether each parameter takes a slot: one named `_` takes none.
    pub params: Vec<bool>,
    /// The slots of the enclosing frame the body reads, a closure inside
    /// it included, in ascending order: what the checker found it
    /// captures.
    pub captures: Vec<usize>,
    /// Whether the body reads `self`, the enclosing method's receiver.
    pub captures_self: bool,
    /// How many slots were in scope where the closure was written: the
    /// first parameter's slot.
    pub height: usize,
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

/// An integer type: `Int`, or a sized one. The one list of the
/// language's integer types and their ranges, for the checker and every
/// backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntType {
    Int,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

impl IntType {
    /// Every integer type, `Int` first.
    pub const ALL: [IntType; 9] = [
        IntType::Int,
        IntType::I8,
        IntType::I16,
        IntType::I32,
        IntType::I64,
        IntType::U8,
        IntType::U16,
        IntType::U32,
        IntType::U64,
    ];

    /// Its name, such as `Int` or `U8`: also its runtime type tag.
    pub fn name(self) -> &'static str {
        match self {
            IntType::Int => "Int",
            IntType::I8 => "I8",
            IntType::I16 => "I16",
            IntType::I32 => "I32",
            IntType::I64 => "I64",
            IntType::U8 => "U8",
            IntType::U16 => "U16",
            IntType::U32 => "U32",
            IntType::U64 => "U64",
        }
    }

    /// The integer type named `name`.
    pub fn named(name: &str) -> Option<IntType> {
        IntType::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Whether it is a sized integer type, `I8` to `U64`, rather than
    /// `Int`.
    pub fn is_sized(self) -> bool {
        self != IntType::Int
    }

    /// The smallest value of the type.
    pub fn min(self) -> i128 {
        match self {
            IntType::Int | IntType::I64 => i64::MIN.into(),
            IntType::I8 => i8::MIN.into(),
            IntType::I16 => i16::MIN.into(),
            IntType::I32 => i32::MIN.into(),
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::U64 => 0,
        }
    }

    /// The largest value of the type.
    pub fn max(self) -> i128 {
        match self {
            IntType::Int | IntType::I64 => i64::MAX.into(),
            IntType::I8 => i8::MAX.into(),
            IntType::I16 => i16::MAX.into(),
            IntType::I32 => i32::MAX.into(),
            IntType::U8 => u8::MAX.into(),
            IntType::U16 => u16::MAX.into(),
            IntType::U32 => u32::MAX.into(),
            IntType::U64 => u64::MAX.into(),
        }
    }
}

/// What a call reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    /// A function in [`Program::functions`], by qualified name.
    Function(String),
    /// A body-less prelude function, which each backend implements, by
    /// qualified name: `Type.method` for a method, such as
    /// `Stdout.println`. A prelude type's implementation of a prelude
    /// trait is named the same way, such as `Int.to_string`.
    Intrinsic(String),
    /// A trait's method called on a value whose type only the runtime
    /// knows: in generic code, a value of a type parameter. The receiver
    /// is the first argument. A backend finds the implementation from the
    /// receiver's runtime type tag: the function `<Tag as Trait>.method`
    /// when the program has one (see [`trait_method`]), and otherwise the
    /// intrinsic `Tag.method` of a prelude implementation. Only a trait
    /// without generics is called this way.
    Trait { trait_name: String, method: String },
}
