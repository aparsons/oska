//! Lowering a checked module into the **checked program**.
//!
//! Checking bodies records what each name and call resolved to in
//! [`Resolutions`], keyed by the span of the node it was resolved at.
//! Lowering walks the AST and reads those records, so it never resolves
//! anything itself. A node it cannot lower yet becomes
//! [`ExprKind::Unsupported`].

use std::collections::HashMap;

use oska_syntax::ast::{
    self, BinaryOp, FnDecl, ItemKind, Module, PatternKind, StmtKind, StrPart, TypeKind, UnaryOp,
};
use oska_syntax::Span;

use super::{
    Arm, Callee, Construct, Expr, ExprKind, Function, IntType, Param, Part, Pattern, Place,
    Program, Stmt,
};
use crate::items::{FnSig, Items};

/// What a node resolved to while its body was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A name that is a local: its index in the body's locals stack,
    /// which is its slot. Recorded at the name.
    Local(usize),
    /// A method call, `receiver.name(args)`, reaching this inherent
    /// method: the receiver is its first argument. Recorded at the call.
    Method(Callee),
    /// A call of a function, or of a method named by its type, as in
    /// `Type.method(receiver, args)`: the arguments as written. Recorded
    /// at the call.
    Call(Callee),
    /// Integer arithmetic, or unary `-`, on this integer type. Recorded at
    /// the operation.
    Int(IntType),
    /// `==` or `!=` on a built-in type, compared by value. Recorded at the
    /// comparison.
    Equality,
    /// An interpolation hole whose type displays through this
    /// implementation. Recorded at the hole.
    Display(Callee),
    /// A constructor call, or a fieldless variant named as a value.
    /// Recorded at the call, or at the variant's name.
    Construct(Construct),
    /// A struct's field, by its position in declaration order. Recorded
    /// at the field's name.
    Field(usize),
}

/// Every resolution in a module, keyed by the span it was recorded at.
pub type Resolutions = HashMap<Span, Resolution>;

/// Lower every free function of `module`, and every method of its
/// inherent impls, as `Type.method`. (Trait impls are not lowered yet.)
pub fn module(module: &Module, items: &Items, resolutions: &Resolutions) -> Program {
    let lowering = Lowering { resolutions };
    let mut program = Program::default();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                let name = f.name.name.clone();
                lowering.function(&mut program, name, f, items.fns.get(&f.name.name));
            }
            ItemKind::Impl(imp) if imp.trait_ref.is_none() => {
                let TypeKind::Path { segments, .. } = &imp.target.kind else {
                    continue;
                };
                let Some(type_name) = segments.last().map(|s| s.name.as_str()) else {
                    continue;
                };
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        let name = format!("{type_name}.{}", f.name.name);
                        let sig = items.inherent_method(type_name, &f.name.name);
                        lowering.function(&mut program, name, f, sig);
                    }
                }
            }
            _ => {}
        }
    }
    program
}

struct Lowering<'a> {
    resolutions: &'a Resolutions,
}

impl Lowering<'_> {
    /// Lower `f`, with signature `sig`, into `program` as `name`, when it
    /// has a body.
    fn function(&self, program: &mut Program, name: String, f: &FnDecl, sig: Option<&FnSig>) {
        let Some(body) = &f.body else {
            return;
        };
        let params = f
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| Param {
                name: p.name.name.clone(),
                ty: sig
                    .and_then(|s| s.params.get(i))
                    .map_or_else(String::new, |(_, ty)| ty.to_string()),
                binds: p.name.name != "_",
            })
            .collect();
        let function = Function {
            name: name.clone(),
            receiver: f.receiver.is_some(),
            params,
            body: self.block(body),
        };
        program.functions.insert(name, function);
    }

    fn block(&self, block: &ast::Block) -> Expr {
        let mut stmts: Vec<Stmt> = Vec::new();
        let mut value = None;
        for (i, stmt) in block.stmts.iter().enumerate() {
            let last = i + 1 == block.stmts.len();
            match &stmt.kind {
                StmtKind::Expr(e) if last => value = Some(Box::new(self.expr(e))),
                _ => stmts.push(self.stmt(stmt)),
            }
        }
        Expr {
            kind: ExprKind::Block { stmts, value },
            span: block.span,
        }
    }

    fn stmt(&self, stmt: &ast::Stmt) -> Stmt {
        match &stmt.kind {
            StmtKind::Expr(e) => Stmt::Expr(self.expr(e)),
            StmtKind::Let { name, value, .. } => Stmt::Let {
                binds: name.name != "_",
                value: self.expr(value),
            },
            StmtKind::Assign { target, value } => match self.place(target) {
                Some(place) => Stmt::Assign {
                    place,
                    value: self.expr(value),
                },
                None => Stmt::Expr(Expr {
                    kind: ExprKind::Unsupported,
                    span: target.span,
                }),
            },
            StmtKind::Return(value) => Stmt::Return(value.as_ref().map(|v| self.expr(v))),
            StmtKind::Break => Stmt::Break,
            StmtKind::Continue => Stmt::Continue,
            StmtKind::While { cond, body } => Stmt::While {
                cond: self.expr(cond),
                body: self.block(body),
            },
            StmtKind::For {
                binding,
                iter,
                body,
            } => Stmt::For {
                binds: binding.name != "_",
                iter: self.expr(iter),
                body: self.block(body),
            },
        }
    }

    /// The place an assignment writes, or `None` when the checked program
    /// cannot express it yet.
    fn place(&self, target: &ast::Expr) -> Option<Place> {
        match &target.kind {
            ast::ExprKind::Name(ident) => match self.resolutions.get(&ident.span) {
                Some(Resolution::Local(slot)) => Some(Place::Local(*slot)),
                _ => None,
            },
            _ => None,
        }
    }

    fn boxed(&self, expr: &ast::Expr) -> Box<Expr> {
        Box::new(self.expr(expr))
    }

    fn expr(&self, expr: &ast::Expr) -> Expr {
        let kind = match &expr.kind {
            ast::ExprKind::Unit => ExprKind::Unit,
            ast::ExprKind::Bool(b) => ExprKind::Bool(*b),
            ast::ExprKind::Int(text) => int_literal(text, false),
            ast::ExprKind::Str(parts) => self.string(parts),
            ast::ExprKind::Name(ident) => self.local(ident),
            ast::ExprKind::Field { base, name } => self.field(expr.span, base, name),
            ast::ExprKind::SelfValue => ExprKind::SelfValue,
            ast::ExprKind::Block(block) => return self.block(block),
            ast::ExprKind::Unary { op, operand } => match (op, &operand.kind) {
                (UnaryOp::Not, _) => ExprKind::Not(self.boxed(operand)),
                // A negative literal is one value, so `-128` fits `I8`.
                (UnaryOp::Neg, ast::ExprKind::Int(text)) => int_literal(text, true),
                (UnaryOp::Neg, _) => match self.resolutions.get(&expr.span) {
                    Some(Resolution::Int(int)) => ExprKind::Neg {
                        operand: self.boxed(operand),
                        int: int.clone(),
                    },
                    _ => ExprKind::Unsupported,
                },
            },
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(expr.span, *op, lhs, rhs),
            ast::ExprKind::Range { start, end } => ExprKind::Range {
                start: self.boxed(start),
                end: self.boxed(end),
            },
            ast::ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => ExprKind::If {
                cond: self.boxed(cond),
                then_branch: Box::new(self.block(then_block)),
                else_branch: else_branch.as_ref().map(|e| self.boxed(e)),
            },
            ast::ExprKind::Call { callee, args } => {
                let args = args.iter().map(|a| self.expr(&a.value));
                match (&callee.kind, self.resolutions.get(&expr.span)) {
                    (ast::ExprKind::Field { base, .. }, Some(Resolution::Method(callee))) => {
                        ExprKind::Call {
                            callee: callee.clone(),
                            args: std::iter::once(self.expr(base)).chain(args).collect(),
                        }
                    }
                    (_, Some(Resolution::Call(callee))) => ExprKind::Call {
                        callee: callee.clone(),
                        args: args.collect(),
                    },
                    (_, Some(Resolution::Construct(ctor))) => ExprKind::Construct {
                        ctor: ctor.clone(),
                        fields: args.collect(),
                    },
                    _ => ExprKind::Unsupported,
                }
            }
            ast::ExprKind::Match { scrutinee, arms } => {
                let arms: Option<Vec<Arm>> = arms.iter().map(|arm| self.arm(arm)).collect();
                match arms {
                    Some(arms) => ExprKind::Match {
                        scrutinee: self.boxed(scrutinee),
                        arms,
                    },
                    None => ExprKind::Unsupported,
                }
            }
            _ => ExprKind::Unsupported,
        };
        Expr {
            kind,
            span: expr.span,
        }
    }

    /// A `match` arm, or `None` when its pattern cannot be lowered yet.
    fn arm(&self, arm: &ast::MatchArm) -> Option<Arm> {
        Some(Arm {
            pattern: self.pattern(&arm.pattern)?,
            guard: arm.guard.as_ref().map(|g| self.expr(g)),
            body: self.expr(&arm.body),
        })
    }

    fn pattern(&self, pattern: &ast::Pattern) -> Option<Pattern> {
        let constructor = || match self.resolutions.get(&pattern.span) {
            Some(Resolution::Construct(ctor)) => Some(ctor.clone()),
            _ => None,
        };
        Some(match &pattern.kind {
            PatternKind::Wildcard => Pattern::Wildcard,
            // An uppercase name is a fieldless variant (ADR 0005).
            PatternKind::Binding(ident) if ident.name.starts_with(|c: char| c.is_ascii_uppercase()) => {
                Pattern::Construct {
                    ctor: constructor()?,
                    fields: Vec::new(),
                }
            }
            PatternKind::Binding(ident) if ident.name == "_" => Pattern::Wildcard,
            PatternKind::Binding(_) => Pattern::Bind,
            PatternKind::Literal(literal) => Pattern::Literal(self.expr(literal)),
            PatternKind::Constructor { args, .. } => Pattern::Construct {
                ctor: constructor()?,
                fields: args
                    .iter()
                    .map(|arg| self.pattern(&arg.pattern))
                    .collect::<Option<_>>()?,
            },
        })
    }

    /// A name in value position: a local, by the slot it resolved to, or
    /// a fieldless variant.
    fn local(&self, ident: &ast::Ident) -> ExprKind {
        match self.resolutions.get(&ident.span) {
            Some(Resolution::Local(slot)) => ExprKind::Local(*slot),
            Some(Resolution::Construct(ctor)) => ExprKind::Construct {
                ctor: ctor.clone(),
                fields: Vec::new(),
            },
            _ => ExprKind::Unsupported,
        }
    }

    /// `base.name` outside a call: a struct's field, or a fieldless
    /// variant such as `Shape.Empty`.
    fn field(&self, span: Span, base: &ast::Expr, name: &ast::Ident) -> ExprKind {
        match (
            self.resolutions.get(&name.span),
            self.resolutions.get(&span),
        ) {
            (Some(Resolution::Field(index)), _) => ExprKind::Field {
                base: self.boxed(base),
                index: *index,
            },
            (_, Some(Resolution::Construct(ctor))) => ExprKind::Construct {
                ctor: ctor.clone(),
                fields: Vec::new(),
            },
            _ => ExprKind::Unsupported,
        }
    }

    fn binary(&self, span: Span, op: BinaryOp, lhs: &ast::Expr, rhs: &ast::Expr) -> ExprKind {
        let resolved = self.resolutions.get(&span);
        let int = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                match resolved {
                    Some(Resolution::Int(int)) => Some(int.clone()),
                    _ => return ExprKind::Unsupported,
                }
            }
            BinaryOp::Eq | BinaryOp::Ne if resolved != Some(&Resolution::Equality) => {
                return ExprKind::Unsupported;
            }
            _ => None,
        };
        ExprKind::Binary {
            op,
            lhs: self.boxed(lhs),
            rhs: self.boxed(rhs),
            int,
        }
    }

    /// A string literal: plain text, or text with holes.
    fn string(&self, parts: &[StrPart]) -> ExprKind {
        match parts {
            [] => return ExprKind::Str(String::new()),
            [StrPart::Text { value, .. }] => return ExprKind::Str(value.clone()),
            _ => {}
        }
        let mut lowered = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                StrPart::Text { value, .. } => lowered.push(Part::Text(value.clone())),
                StrPart::Hole { path, span } => {
                    let (Some(Resolution::Display(display)), Some(value)) =
                        (self.resolutions.get(span), self.hole(path))
                    else {
                        return ExprKind::Unsupported;
                    };
                    lowered.push(Part::Hole {
                        value,
                        display: display.clone(),
                    });
                }
            }
        }
        ExprKind::Interpolated(lowered)
    }

    /// A hole's path, `name` or `name.field...`, as the expression that
    /// reads it.
    fn hole(&self, path: &[ast::Ident]) -> Option<Expr> {
        let (first, fields) = path.split_first()?;
        let mut value = Expr {
            kind: self.local(first),
            span: first.span,
        };
        for name in fields {
            let Some(Resolution::Field(index)) = self.resolutions.get(&name.span) else {
                return None;
            };
            value = Expr {
                kind: ExprKind::Field {
                    base: Box::new(value),
                    index: *index,
                },
                span: Span {
                    start: first.span.start,
                    end: name.span.end,
                },
            };
        }
        Some(value)
    }
}

/// An integer literal as written (digits and `_` separators), negated
/// when `negative`. The checker has already checked that it fits its
/// type.
fn int_literal(text: &str, negative: bool) -> ExprKind {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    match digits.parse::<i128>() {
        Ok(value) if negative => ExprKind::Int(-value),
        Ok(value) => ExprKind::Int(value),
        Err(_) => ExprKind::Unsupported,
    }
}
