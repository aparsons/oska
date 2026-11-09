//! Syntax tree for Oska source. Every node carries the [`Span`] of source
//! text it was parsed from, so diagnostics and machine-applicable fixes can
//! point at exact byte ranges.
//!
//! The parser builds paths such as `Shape.Circle` or `fs.read_to_string`
//! as plain [`ExprKind::Field`] chains. Name resolution, not the parser,
//! decides which segments are modules, types, variants, or fields.

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    /// Integer literal, kept as written (digits and `_` separators).
    Int(String),
    /// String literal, split into text and interpolation holes.
    Str(Vec<StrPart>),
    /// Sink literal such as `sql"..."`. It never contains holes.
    SinkStr {
        tag: String,
        value: String,
    },
    Bool(bool),
    /// The unit value `()`.
    Unit,
    Name(Ident),
    /// The receiver `self`.
    SelfValue,
    /// `base.name`: a field, method, module member, or enum variant.
    Field {
        base: Box<Expr>,
        name: Ident,
    },
    /// `callee(args)`. A method call is a call whose callee is a `Field`.
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Postfix `?`.
    Try(Box<Expr>),
    /// `start..end`, which excludes `end`.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    /// `[a, b, c]`.
    List(Vec<Expr>),
    /// `|params| body`. Whether a closure is capability-bearing is
    /// inferred later, not written here.
    Closure {
        params: Vec<ClosureParam>,
        body: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_block: Block,
        /// Either another `If` or a `Block` expression.
        else_branch: Option<Box<Expr>>,
    },
    Block(Block),
    /// `scope |handle| { ... }`.
    Scope {
        handle: Ident,
        body: Block,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Implies,
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinaryOp {
    /// The operator as written in source.
    pub fn text(self) -> &'static str {
        match self {
            Self::Implies => "implies",
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }
}

/// A call argument with an optional label: `copy(src: a, dst: b)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub label: Option<Ident>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrPart {
    /// Literal text with escapes resolved.
    Text(String),
    /// An interpolation hole: a name or field path, such as `{user.name}`.
    Hole { path: Vec<Ident>, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<Type>,
}

/// A `{ ... }` block. Its value is the value of its last statement if that
/// statement is an expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StmtKind {
    /// `let` (immutable) or `var` (mutable). The initializer is required.
    Let {
        mutable: bool,
        name: Ident,
        ty: Option<Type>,
        value: Expr,
    },
    /// `target = value`. `target` is a name, `self`, or a field chain.
    Assign {
        target: Expr,
        value: Expr,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    For {
        binding: Ident,
        iter: Expr,
        body: Block,
    },
    While {
        cond: Expr,
        body: Block,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    /// `a.b.C<T, U>`.
    Path {
        segments: Vec<Ident>,
        args: Vec<Type>,
    },
    /// `()`.
    Unit,
    /// `fn(T) -> U`, or `cap fn(T) -> U` when it may capture capabilities.
    /// A missing return type means `()`.
    Fn {
        cap: bool,
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
}
