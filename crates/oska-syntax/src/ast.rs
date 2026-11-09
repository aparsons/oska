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
    /// Literal text with escapes resolved. The span covers the raw source
    /// text, escapes included.
    Text { value: String, span: Span },
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

/// A whole source file: its items in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub items: Vec<Item>,
    pub span: Span,
}

/// A top-level declaration. Modules contain only items: there is no
/// module-level code that runs on load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Declared `pub`.
    pub public: bool,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
}

/// `fn name<generics>(params) -> ret requires ... ensures |r| ... { body }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    /// The `self` or `var self` receiver, which can only come first.
    pub receiver: Option<SelfParam>,
    /// The parameters after the receiver.
    pub params: Vec<Param>,
    /// A missing return type means `()`.
    pub ret: Option<Type>,
    /// `requires` clauses, in source order.
    pub requires: Vec<Requires>,
    /// `ensures |r| ...` clauses, in source order.
    pub ensures: Vec<Ensures>,
    pub body: Block,
}

/// A method's receiver: `self`, or `var self` for a mutating method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfParam {
    /// `true` for `var self`.
    pub mutable: bool,
    pub span: Span,
}

/// A generic parameter with its inline bounds: `T: Eq + Hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub name: Ident,
    /// Empty when the parameter is unbounded.
    pub bounds: Vec<Type>,
    pub span: Span,
}

/// A `requires cond` clause. The span covers the keyword and condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requires {
    pub cond: Expr,
    pub span: Span,
}

/// An `ensures |binding| cond` clause, where `binding` names the return
/// value. The span covers the keyword and condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ensures {
    pub binding: Ident,
    pub cond: Expr,
    pub span: Span,
}

/// A function parameter, `name: Type`. Capabilities are ordinary
/// parameters, so a function's authority can be read from these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

/// `struct Name { fields }` or `struct Name(fields)`, optionally `cap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    /// Declared `cap struct`: the type may hold capabilities.
    pub cap: bool,
    pub name: Ident,
    pub fields: Fields,
}

/// The fields of a struct or enum variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fields {
    /// `{ a: T, b: U }`. Every field has a name.
    Named(Vec<Field>),
    /// `(T, U)`. No field has a name.
    Positional(Vec<Field>),
    /// No fields at all, as in the variant `None`. Only enum variants
    /// take this form.
    Unit,
}

/// A declared field: `pub name: Type`, or just `Type` when positional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Declared `pub`. Fields are private by default.
    pub public: bool,
    /// Present for named fields, absent for positional ones.
    pub name: Option<Ident>,
    pub ty: Type,
    pub span: Span,
}

/// `enum Name { variants }`, optionally `cap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    /// Declared `cap enum`: the type may hold capabilities.
    pub cap: bool,
    pub name: Ident,
    pub variants: Vec<Variant>,
}

/// An enum variant: `Some(T)`, `Circle { radius: Int }`, or `None`. Its
/// fields never carry `pub`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: Ident,
    pub fields: Fields,
    pub span: Span,
}
