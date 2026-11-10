//! Lowering a checked module into the **checked program**.
//!
//! Checking bodies records what each name and call resolved to in
//! [`Resolutions`], keyed by the span of the node it was resolved at.
//! Lowering walks the AST and reads those records, so it never resolves
//! anything itself. A node it cannot lower yet becomes
//! [`ExprKind::Unsupported`], named by the checker's
//! [`Resolution::Unsupported`] when it recorded one, and by what the node
//! is otherwise.

use std::collections::HashMap;
use std::sync::Arc;

use oska_syntax::ast::{
    self, BinaryOp, FnDecl, Item, ItemKind, Module, PatternKind, StmtKind, StrPart, TypeKind,
    UnaryOp,
};
use oska_syntax::Span;

use super::{
    Arm, Callee, Closure, Construct, Expr, ExprKind, Function, IntType, Param, Part, Pattern,
    Place, Program, Stmt,
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
    /// A method call reaching this inherent **mutating method**
    /// (`var self`), which writes its receiver back. Recorded at the
    /// call.
    MutatingMethod(Callee),
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
    /// A call of a function value, such as a closure. Recorded at the
    /// call.
    CallValue,
    /// A struct's field, by its position in declaration order. Recorded
    /// at the field's name.
    Field(usize),
    /// `?`, converting an error through this `From` implementation, or
    /// passing it on as it is. Recorded at the `?` expression.
    Try(Option<Callee>),
    /// A function named as a value. Recorded at the name.
    FnValue(Callee),
    /// `==` or `!=` through this `Eq` implementation, on a type that is
    /// not built in. Recorded at the comparison.
    EqualityVia(Callee),
    /// Something the checked program cannot express yet, named for the
    /// refusal, such as "a `From`-bound conversion": recorded where the
    /// checker knows why. Recorded at the construct.
    Unsupported(String),
    /// A closure: the slots it captures, ascending, whether it captures
    /// `self`, and the number of slots in scope where it starts. Recorded
    /// at the closure.
    Closure {
        captures: Vec<usize>,
        captures_self: bool,
        height: usize,
    },
}

/// Every resolution in a module, keyed by the span it was recorded at.
pub type Resolutions = HashMap<Span, Resolution>;

/// Lower every free function of `module`, every method of its inherent
/// impls, as `Type.method`, and every method of its trait impls, as
/// `<Type as Trait>.method`: the trait's default methods the impl does
/// not override included, so every impl is complete.
pub fn module(module: &Module, items: &Items, resolutions: &Resolutions) -> Program {
    let lowering = Lowering { resolutions };
    let mut program = Program::default();
    // Each trait the module declares, by name, for its default methods.
    let traits: HashMap<&str, &[Item]> = module
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Trait(t) => Some((t.name.name.as_str(), t.methods.as_slice())),
            _ => None,
        })
        .collect();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                let name = f.name.name.clone();
                lowering.function(&mut program, name, f, items.fns.get(&f.name.name));
            }
            ItemKind::Use(u) => {
                if let (Some(first), Some(last)) = (u.path.first(), u.path.last()) {
                    let path: Vec<&str> = u.path.iter().map(|s| s.name.as_str()).collect();
                    let span = Span {
                        start: first.span.start,
                        end: last.span.end,
                    };
                    program.imports.push((path.join("."), span));
                }
            }
            ItemKind::Impl(imp) => {
                let Some(type_name) = path_name(&imp.target) else {
                    continue;
                };
                let trait_impl = match &imp.trait_ref {
                    Some(_) => match items.impls.iter().find(|i| i.span == Some(item.span)) {
                        Some(info) => Some(info),
                        None => continue,
                    },
                    None => None,
                };
                let mut methods: Vec<&FnDecl> = fn_decls(&imp.methods).collect();
                // A default method the impl does not override is the
                // impl's too: its body sees `self` as the impl's type.
                if let Some(defaults) = trait_impl.and_then(|i| traits.get(i.trait_name.as_str())) {
                    let defaults: Vec<&FnDecl> = fn_decls(defaults)
                        .filter(|d| !methods.iter().any(|m| m.name.name == d.name.name))
                        .collect();
                    methods.extend(defaults);
                }
                for f in methods {
                    let method = &f.name.name;
                    let (name, sig) = match trait_impl {
                        Some(info) => (
                            match info.method_name(method) {
                                Some(name) => name,
                                None => continue,
                            },
                            items
                                .trait_methods_named(type_name, method)
                                .into_iter()
                                .find(|s| s.trait_name.as_deref() == Some(&info.trait_name)),
                        ),
                        None => (
                            format!("{type_name}.{method}"),
                            items.inherent_method(type_name, method),
                        ),
                    };
                    lowering.function(&mut program, name, f, sig);
                }
            }
            ItemKind::Test(test) => program.tests.push(super::Test {
                name: test_name(&test.name),
                integration: item.attributes.iter().any(|a| a.name.name == "integration"),
                body: lowering.block(&test.body),
                span: item.span,
            }),
            _ => {}
        }
    }
    program
}

/// A test's name: the parser allows only a plain string literal.
fn test_name(name: &ast::Expr) -> String {
    match &name.kind {
        ast::ExprKind::Str(parts) => parts
            .iter()
            .map(|part| match part {
                StrPart::Text { value, .. } => value.as_str(),
                StrPart::Hole { .. } => "",
            })
            .collect(),
        _ => String::new(),
    }
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
                    kind: self.unsupported(target.span, "an assignment to this place"),
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
            ast::ExprKind::SelfValue => Some(Place::SelfValue),
            ast::ExprKind::Field { base, name } => match self.resolutions.get(&name.span) {
                Some(Resolution::Field(index)) => Some(Place::Field {
                    base: Box::new(self.place(base)?),
                    index: *index,
                }),
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
            ast::ExprKind::Int(text) => self.int_literal(text, false, expr.span),
            ast::ExprKind::Str(parts) => self.string(parts),
            ast::ExprKind::Name(ident) => self.local(ident),
            ast::ExprKind::Field { base, name } => self.field(expr.span, base, name),
            ast::ExprKind::SelfValue => ExprKind::SelfValue,
            ast::ExprKind::Block(block) => return self.block(block),
            ast::ExprKind::Unary { op, operand } => match (op, &operand.kind) {
                (UnaryOp::Not, _) => ExprKind::Not(self.boxed(operand)),
                // A negative literal is one value, so `-128` fits `I8`.
                (UnaryOp::Neg, ast::ExprKind::Int(text)) => self.int_literal(text, true, expr.span),
                (UnaryOp::Neg, _) => match self.resolutions.get(&expr.span) {
                    Some(Resolution::Int(int)) => ExprKind::Neg {
                        operand: self.boxed(operand),
                        int: *int,
                    },
                    _ => self.unsupported(expr.span, "`-` on this type"),
                },
            },
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(expr.span, *op, lhs, rhs),
            ast::ExprKind::Closure { params, body } => match self.resolutions.get(&expr.span) {
                Some(Resolution::Closure {
                    captures,
                    captures_self,
                    height,
                }) => ExprKind::Closure(Arc::new(Closure {
                    params: params.iter().map(|p| p.name.name != "_").collect(),
                    captures: captures.clone(),
                    captures_self: *captures_self,
                    height: *height,
                    body: self.expr(body),
                })),
                _ => self.unsupported(expr.span, "this closure"),
            },
            ast::ExprKind::List(items) => {
                ExprKind::List(items.iter().map(|item| self.expr(item)).collect())
            }
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
                    (
                        ast::ExprKind::Field { base, .. },
                        Some(Resolution::MutatingMethod(callee)),
                    ) => match self.place(base) {
                        Some(receiver) => ExprKind::MutatingCall {
                            callee: callee.clone(),
                            receiver,
                            args: args.collect(),
                        },
                        // A temporary, such as a call's result: the
                        // change is not kept.
                        None => ExprKind::Call {
                            callee: callee.clone(),
                            args: std::iter::once(self.expr(base)).chain(args).collect(),
                        },
                    },
                    (_, Some(Resolution::Call(callee))) => ExprKind::Call {
                        callee: callee.clone(),
                        args: args.collect(),
                    },
                    (_, Some(Resolution::CallValue)) => ExprKind::CallValue {
                        callee: self.boxed(callee),
                        args: args.collect(),
                    },
                    (_, Some(Resolution::Construct(ctor))) => ExprKind::Construct {
                        ctor: ctor.clone(),
                        fields: args.collect(),
                    },
                    _ => self.unsupported(expr.span, "this call"),
                }
            }
            ast::ExprKind::Try(operand) => match self.resolutions.get(&expr.span) {
                Some(Resolution::Try(convert)) => ExprKind::Try {
                    operand: self.boxed(operand),
                    convert: convert.clone(),
                },
                _ => self.unsupported(expr.span, "this `?`"),
            },
            ast::ExprKind::Scope { body, .. } => ExprKind::Scope(Box::new(self.block(body))),
            ast::ExprKind::Match { scrutinee, arms } => {
                let arms: Option<Vec<Arm>> = arms.iter().map(|arm| self.arm(arm)).collect();
                match arms {
                    Some(arms) => ExprKind::Match {
                        scrutinee: self.boxed(scrutinee),
                        arms,
                    },
                    None => self.unsupported(expr.span, "a pattern in this `match`"),
                }
            }
            ast::ExprKind::SinkStr { tag, .. } => {
                self.unsupported(expr.span, &format!("a `{tag}\"...\"` literal"))
            }
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
            PatternKind::Binding(ident)
                if ident.name.starts_with(|c: char| c.is_ascii_uppercase()) =>
            {
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

    /// A name in value position: a local, by the slot it resolved to, a
    /// fieldless variant, or a function.
    fn local(&self, ident: &ast::Ident) -> ExprKind {
        match self.resolutions.get(&ident.span) {
            Some(Resolution::Local(slot)) => ExprKind::Local(*slot),
            Some(Resolution::FnValue(callee)) => ExprKind::FnValue(callee.clone()),
            Some(Resolution::Construct(ctor)) => ExprKind::Construct {
                ctor: ctor.clone(),
                fields: Vec::new(),
            },
            _ => self.unsupported(ident.span, &format!("`{}` as a value", ident.name)),
        }
    }

    /// An integer literal as written (digits and `_` separators), negated
    /// when `negative`, of the type the checker recorded at `span`. The
    /// checker has already checked that it fits.
    fn int_literal(&self, text: &str, negative: bool, span: Span) -> ExprKind {
        let Some(Resolution::Int(int)) = self.resolutions.get(&span) else {
            return self.unsupported(span, "an integer literal of a type not known");
        };
        let digits: String = text.chars().filter(|&c| c != '_').collect();
        match digits.parse::<i128>() {
            Ok(value) if negative => ExprKind::Int(-value, *int),
            Ok(value) => ExprKind::Int(value, *int),
            Err(_) => self.unsupported(span, "an integer literal this large"),
        }
    }

    /// The node at `span` the checked program cannot express: named as
    /// the checker recorded it, or else as `what`.
    fn unsupported(&self, span: Span, what: &str) -> ExprKind {
        match self.resolutions.get(&span) {
            Some(Resolution::Unsupported(named)) => ExprKind::Unsupported(named.clone()),
            _ => ExprKind::Unsupported(what.to_string()),
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
            (Some(Resolution::FnValue(callee)), _) => ExprKind::FnValue(callee.clone()),
            (_, Some(Resolution::Construct(ctor))) => ExprKind::Construct {
                ctor: ctor.clone(),
                fields: Vec::new(),
            },
            _ => self.unsupported(span, &format!("`.{}` here", name.name)),
        }
    }

    fn binary(&self, span: Span, op: BinaryOp, lhs: &ast::Expr, rhs: &ast::Expr) -> ExprKind {
        let resolved = self.resolutions.get(&span);
        if let (BinaryOp::Eq | BinaryOp::Ne, Some(Resolution::EqualityVia(eq))) = (op, resolved) {
            let call = ExprKind::Call {
                callee: eq.clone(),
                args: vec![self.expr(lhs), self.expr(rhs)],
            };
            if op == BinaryOp::Eq {
                return call;
            }
            return ExprKind::Not(Box::new(Expr { kind: call, span }));
        }
        let int = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                match resolved {
                    Some(Resolution::Int(int)) => Some(*int),
                    _ => return self.unsupported(span, &format!("`{}` on this type", op.text())),
                }
            }
            BinaryOp::Eq | BinaryOp::Ne if resolved != Some(&Resolution::Equality) => {
                return self.unsupported(span, &format!("`{}` on this type", op.text()));
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
                        return self.unsupported(*span, "this interpolation hole");
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

/// The functions among `items`, the methods of an impl or a trait.
fn fn_decls(items: &[Item]) -> impl Iterator<Item = &FnDecl> {
    items.iter().filter_map(|item| match &item.kind {
        ItemKind::Fn(f) => Some(f),
        _ => None,
    })
}

/// The last segment of a path type, such as `Point` or `Display`.
fn path_name(ty: &ast::Type) -> Option<&str> {
    match &ty.kind {
        TypeKind::Path { segments, .. } => segments.last().map(|s| s.name.as_str()),
        _ => None,
    }
}
