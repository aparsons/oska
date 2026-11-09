//! Checking function bodies: local, bidirectional type inference.
//!
//! Each body gets a fresh [`Body`] with its own inference context, so
//! nothing is inferred across function boundaries. Every expression is
//! checked by [`Body::expr`], which takes an optional expected type and
//! returns the expression's type:
//!
//! - [`Body::check`] checks an expression against a type it must have,
//!   reporting a single mismatch (`E0220`) when it doesn't.
//! - [`Body::infer`] finds an expression's type with no expectation.
//!
//! An expression that fails gets [`Ty::Error`], which unifies with
//! everything, so one mistake produces one diagnostic.
//!
//! Extension points for later stages:
//!
//! - **Calls and methods:** in `body/calls.rs`. Every callee becomes a
//!   `Callable`, applied by [`Body::apply`]. Method calls
//!   ([`Body::method_call`]) try the type's own `impl`, then its traits;
//!   on a generic parameter, only its bounds' traits.
//! - **`?` and the `Result` discipline:** in `body/results.rs`.
//!   [`Body::try_expr`] converts errors when the function's error type
//!   implements `From<E>` ([`Body::converts_from`]), and
//!   [`Body::expr_stmt`] sees every expression whose value is discarded.
//! - **Patterns and `match`:** in `body/patterns.rs`. Patterns are checked
//!   against the scrutinee's type, then lowered for the usefulness check
//!   (`usefulness.rs`) that finds missing and unreachable arms.
//! - **Traits:** [`Body::implements`] answers whether a type implements a
//!   trait, through a generic parameter's bounds or an `impl`.
//! - **Closures:** [`Body::closure`]. Locals are a stack, so a closure's
//!   captures are the names it resolves below the stack height at which
//!   the closure's parameters were pushed (plus `self`). Each closure
//!   being checked has a frame recording them; a closure that captures a
//!   capability-bearing value is a `cap fn`. Unification itself enforces
//!   that a `cap fn` never flows into a plain `fn` (`infer.rs`), so every
//!   route a value takes is covered; [`Body::mismatch`] reports it.

use oska_syntax::ast::{
    BinaryOp, Block, ClosureParam, Expr, ExprKind, FnDecl, Ident, ItemKind, Module, StmtKind,
    StrPart, UnaryOp,
};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::authority::capability_bearing;
use crate::codes;
use crate::collect::Decls;
use crate::decl_rules;
use crate::infer::{Infer, Mismatch};
use crate::items::{Bound, Items, Lower};
use crate::suggest;
use crate::ty::{self, Ty};

mod calls;
mod patterns;
mod results;

/// Check every function, method, constant, and test body in `module`. A
/// test body sees `t`, and nothing else beyond pure code.
pub fn module(
    src: &str,
    module: &Module,
    decls: &Decls,
    items: &Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => function(src, f, Lower::new(decls), items, diagnostics),
            ItemKind::Impl(imp) => {
                let lower = Lower::new(decls).with_generics(&imp.generics);
                let target = lower.ty(&imp.target);
                let lower = lower.with_self(target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        function(src, f, lower.clone(), items, diagnostics);
                    }
                }
            }
            ItemKind::Trait(t) => {
                // Inside its own trait, `Self` implements the trait.
                let lower = Lower::new(decls)
                    .with_generics(&t.generics)
                    .with_self(Ty::Param(ty::SELF.to_string()))
                    .with_bound(Bound {
                        param: ty::SELF.to_string(),
                        trait_name: t.name.name.clone(),
                        args: t
                            .generics
                            .iter()
                            .map(|g| Ty::Param(g.name.name.clone()))
                            .collect(),
                    });
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        function(src, f, lower.clone(), items, diagnostics);
                    }
                }
            }
            ItemKind::Const(c) => {
                let lower = Lower::new(decls);
                let ty = lower.ty(&c.ty);
                let mut body = Body::new(src, items, lower, Ty::Unit, diagnostics);
                body.check(&c.value, &ty);
            }
            ItemKind::Test(test) => {
                let mut body = Body::new(src, items, Lower::new(decls), Ty::Unit, diagnostics);
                body.bind("t", Ty::named(ty::TEST_CONTEXT), Binding::Param);
                // A test produces no value: its last expression is a
                // statement like the others.
                body.block(&test.body, None, false);
                body.finish();
            }
            ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Use(_) => {}
        }
    }
}

/// Check one function body against its signature. `lower` has the
/// enclosing impl's or trait's generics and `Self` in scope.
fn function(src: &str, f: &FnDecl, lower: Lower, items: &Items, diagnostics: &mut Vec<Diagnostic>) {
    if f.body.is_none() && f.requires.is_empty() && f.ensures.is_empty() {
        return;
    }
    let lower = lower.with_generics(&f.generics);
    let sig = lower.signature(f, &[], lower.self_ty.clone());
    let mut body = Body::new(src, items, lower, sig.ret.clone(), diagnostics);
    if let Some(receiver) = &f.receiver {
        body.receiver = Some(SelfBinding {
            ty: sig.self_ty.clone().unwrap_or(Ty::Error),
            mutable: receiver.mutable,
            span: receiver.span,
        });
    }
    for (name, ty) in &sig.params {
        body.bind(name, ty.clone(), Binding::Param);
    }
    let ret = sig.ret.clone();
    for clause in &f.requires {
        body.contract("requires", &clause.cond);
    }
    for clause in &f.ensures {
        let height = body.locals.len();
        body.bind(&clause.binding.name, ret.clone(), Binding::Param);
        body.contract("ensures", &clause.cond);
        body.locals.truncate(height);
    }
    if let Some(block) = &f.body {
        body.block(block, Some(&ret), true);
    }
    body.finish();
}

/// How a local was introduced, which decides whether it can be mutated.
#[derive(Debug, Clone, Copy)]
enum Binding {
    /// `let`, with the span of the `let` keyword for the fix to `var`.
    Let(Span),
    Var,
    Param,
    /// A `for` loop variable.
    Loop,
    /// Bound by a `match` pattern or a `scope` handle.
    Pattern,
}

#[derive(Debug)]
struct Local {
    name: String,
    ty: Ty,
    binding: Binding,
}

/// The method's receiver, `self` or `var self`.
#[derive(Debug)]
struct SelfBinding {
    ty: Ty,
    mutable: bool,
    span: Span,
}

/// How a call's argument labels are checked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Labels {
    /// A function call: labels are optional and must match the parameter.
    Optional,
    /// Construction of named fields: every label is required, in order.
    Required,
    /// Construction of positional fields: no labels.
    None,
}

/// A callable thing, already instantiated: its parameters (with names,
/// when it has them), result type, and how labels are checked.
struct Callable {
    description: String,
    params: Vec<(Option<String>, Ty)>,
    ret: Ty,
    labels: Labels,
    /// The callee's bounds, instantiated: each type must implement its
    /// bound's trait. (The bound's `param` names the generic it came
    /// from.)
    obligations: Vec<(Ty, Bound)>,
    /// A pure function as declared: no parameter, the receiver included,
    /// is capability-bearing. Constructors are pure; a closure is pure
    /// unless it is a `cap fn`. A generic callee can still be impure at a
    /// call, once its parameters are instantiated (see [`Body::apply`]).
    pure: bool,
    /// For a method: the type of its receiver, instantiated.
    receiver: Option<Ty>,
}

/// The state of checking one body.
pub struct Body<'c, 'd, 'a> {
    /// The module's source, for fixes that match its layout.
    src: &'c str,
    items: &'c Items,
    lower: Lower<'d, 'a>,
    infer: Infer,
    diagnostics: &'c mut Vec<Diagnostic>,
    /// Locals in scope, innermost last. Shadowing pushes a new one.
    locals: Vec<Local>,
    receiver: Option<SelfBinding>,
    /// The type `return` must produce: the function's or the innermost
    /// closure's.
    ret: Ty,
    loop_depth: usize,
    /// Unannotated `let` and `var` bindings, checked for unsolved types at
    /// the end of the body.
    inferred_lets: Vec<(Ident, Ty)>,
    /// The closures being checked, innermost last.
    closures: Vec<ClosureFrame>,
    /// Checking a contract clause, where only pure functions may be
    /// called.
    in_contract: bool,
}

/// A closure being checked, and what it captures so far.
struct ClosureFrame {
    /// The number of locals in scope where the closure starts: a local
    /// below this height is a capture.
    height: usize,
    /// Each captured name (`self` for the receiver) with its type.
    captures: Vec<(String, Ty)>,
}

impl<'c, 'd, 'a> Body<'c, 'd, 'a> {
    fn new(
        src: &'c str,
        items: &'c Items,
        lower: Lower<'d, 'a>,
        ret: Ty,
        diagnostics: &'c mut Vec<Diagnostic>,
    ) -> Self {
        Body {
            src,
            items,
            lower,
            infer: Infer::default(),
            diagnostics,
            locals: Vec::new(),
            receiver: None,
            ret,
            loop_depth: 0,
            inferred_lets: Vec::new(),
            closures: Vec::new(),
            in_contract: false,
        }
    }

    fn decls(&self) -> &'d Decls<'a> {
        self.lower.decls()
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(code, message, span));
    }

    /// Report bindings whose types inference could not work out.
    fn finish(&mut self) {
        for (name, ty) in std::mem::take(&mut self.inferred_lets) {
            let ty = self.infer.resolve(&ty);
            if ty.has_vars() {
                self.error(
                    codes::CANNOT_INFER,
                    format!(
                        "cannot infer the type of `{}` (`{ty}`); add a type annotation",
                        name.name
                    ),
                    name.span,
                );
            }
        }
    }

    // ---- Contracts ------------------------------------------------------

    /// A `requires` or `ensures` condition: it must be `Bool`, and while
    /// it is checked only pure functions may be called.
    fn contract(&mut self, keyword: &str, cond: &Expr) {
        let outer = std::mem::replace(&mut self.in_contract, true);
        let ty = self.infer(cond);
        self.in_contract = outer;
        if !self.infer.unify(&ty, &Ty::bool()) {
            let ty = self.infer.resolve(&ty);
            self.error(
                codes::CONTRACT_NOT_BOOL,
                format!("a `{keyword}` condition must be `Bool`, found `{ty}`"),
                cond.span,
            );
        }
    }

    // ---- Locals ---------------------------------------------------------

    fn bind(&mut self, name: &str, ty: Ty, binding: Binding) {
        if name != "_" {
            self.locals.push(Local {
                name: name.to_string(),
                ty,
                binding,
            });
        }
    }

    /// A local's name must start lowercase (ADR 0005). A pattern's
    /// uppercase names are variants, so this is for `let`, `var`, and `for`.
    fn require_lowercase(&mut self, name: &Ident) {
        if name.name.starts_with(|c: char| c.is_ascii_uppercase()) {
            self.error(
                codes::NOT_LOWERCASE,
                format!(
                    "variable name `{}` must start with a lowercase letter",
                    name.name
                ),
                name.span,
            );
        }
    }

    fn local(&self, name: &str) -> Option<&Local> {
        self.locals.iter().rev().find(|l| l.name == name)
    }

    /// `name` is a type in value position, such as `Shape` in
    /// `Shape.Circle`, and not shadowed by a local.
    fn type_name(&self, expr: &Expr) -> Option<String> {
        let ExprKind::Name(ident) = &expr.kind else {
            return None;
        };
        if self.local(&ident.name).is_some() {
            return None;
        }
        self.decls().get(&ident.name).map(|_| ident.name.clone())
    }

    // ---- Expressions ----------------------------------------------------

    /// Check `expr` against `expected`, reporting a mismatch. Returns the
    /// expression's type, or [`Ty::Error`] after a mismatch.
    pub fn check(&mut self, expr: &Expr, expected: &Ty) -> Ty {
        let ty = self.expr(expr, Some(expected));
        self.expect(&ty, expected, expr.span)
    }

    /// The type of `expr`, with no expectation.
    pub fn infer(&mut self, expr: &Expr) -> Ty {
        self.expr(expr, None)
    }

    /// Unify `actual` with `expected`, reporting `E0220` at `span` when
    /// they differ.
    fn expect(&mut self, actual: &Ty, expected: &Ty, span: Span) -> Ty {
        match self.infer.flow(actual, expected) {
            Ok(()) => actual.clone(),
            Err(mismatch) => {
                self.mismatch(mismatch, actual, expected, span);
                Ty::Error
            }
        }
    }

    /// Report that `actual` cannot flow into `expected`: `E0270` when only
    /// the `cap` rule stops it, `E0300` when only the trust rule does,
    /// `E0220` otherwise.
    fn mismatch(&mut self, mismatch: Mismatch, actual: &Ty, expected: &Ty, span: Span) {
        let expected = self.infer.resolve(expected);
        let actual = self.infer.resolve(actual);
        match mismatch {
            Mismatch::CapLost => self.error(
                codes::CAPTURING_CLOSURE,
                format!(
                    "expected `{expected}`, found `{actual}`: a `cap fn` can carry authority, \
                     so it cannot be used as a plain `fn`"
                ),
                span,
            ),
            Mismatch::TrustLost => self.error(
                codes::UNTRUSTED_TEXT,
                format!(
                    "expected `{expected}`, found `{actual}`: untrusted text cannot be used \
                     where trusted text is required; check it with a validator, or vouch for \
                     it with `.assume_trusted()`"
                ),
                span,
            ),
            Mismatch::Types => self.error(
                codes::MISMATCH,
                format!("expected `{expected}`, found `{actual}`"),
                span,
            ),
        }
    }

    /// The type of `expr`. `expected`, when given, guides inference (an
    /// integer literal takes it, a closure's parameters come from it), but
    /// is not enforced: [`Body::check`] does that.
    pub fn expr(&mut self, expr: &Expr, expected: Option<&Ty>) -> Ty {
        match &expr.kind {
            ExprKind::Int(text) => {
                let ty = self.literal_type(expected);
                self.range_check(text, false, &ty, expr.span);
                ty
            }
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Hole { path, .. } = part {
                        self.hole(path);
                    }
                }
                Ty::string()
            }
            // Sink literals get their types with untrusted values (#8).
            ExprKind::SinkStr { .. } => Ty::Error,
            ExprKind::Bool(_) => Ty::bool(),
            ExprKind::Unit => Ty::Unit,
            ExprKind::Name(ident) => self.name(ident),
            ExprKind::SelfValue => match &self.receiver {
                Some(receiver) => {
                    let ty = receiver.ty.clone();
                    for frame in &mut self.closures {
                        frame.captures.push(("self".to_string(), ty.clone()));
                    }
                    ty
                }
                None => {
                    self.error(
                        codes::UNKNOWN_VALUE,
                        "`self` is only available in a method with a `self` receiver",
                        expr.span,
                    );
                    Ty::Error
                }
            },
            ExprKind::Field { base, name } => self.field_expr(base, name),
            ExprKind::Call { callee, args } => self.call(expr, callee, args, expected),
            ExprKind::Unary { op, operand } => self.unary(expr, *op, operand, expected),
            ExprKind::Binary { op, lhs, rhs } => self.binary(expr, *op, lhs, rhs, expected),
            ExprKind::Try(operand) => self.try_expr(expr, operand),
            ExprKind::Range { start, end } => {
                let ty = self.infer(start);
                let ty = self.check(end, &ty);
                let ty = self.infer.shallow(&ty);
                if !ty.is_error() && !ty.is_integer() {
                    self.error(
                        codes::BAD_OPERAND,
                        format!("`..` needs integer bounds, found `{ty}`"),
                        expr.span,
                    );
                    return Ty::Error;
                }
                Ty::range(ty)
            }
            ExprKind::List(elements) => {
                let expected = expected.map(|e| self.infer.shallow(e));
                let element = match expected.as_ref().and_then(|e| e.args_of(ty::LIST, 1)) {
                    Some([element]) => element.clone(),
                    _ => self.infer.fresh(),
                };
                for e in elements {
                    self.check(e, &element);
                }
                Ty::list(element)
            }
            ExprKind::Closure { params, body } => self.closure(params, body, expected, expr.span),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.check(cond, &Ty::bool());
                let Some(else_branch) = else_branch else {
                    // Without `else` the value is `()`, and the block's own
                    // value is discarded.
                    self.block(then_block, None, false);
                    return Ty::Unit;
                };
                let then = self.block(then_block, expected, true);
                match expected {
                    Some(expected) => self.check(else_branch, expected),
                    None if self.infer.shallow(&then) == Ty::Never => self.infer(else_branch),
                    None => self.check(else_branch, &then),
                }
            }
            ExprKind::Block(block) => self.block(block, expected, true),
            ExprKind::Scope { handle, body } => {
                // The handle is the prelude's `Scope`; its `spawn` and the
                // task's `join` are ordinary prelude methods.
                self.locals.push(Local {
                    name: handle.name.clone(),
                    ty: Ty::named(ty::SCOPE),
                    binding: Binding::Pattern,
                });
                let ty = self.block(body, expected, true);
                self.locals.pop();
                if self.infer.resolve(&ty).holds(ty::SCOPE) {
                    self.diagnostics.push(decl_rules::scope_escapes(
                        expr.span,
                        "be the value of its `scope` block",
                    ));
                    return Ty::Error;
                }
                ty
            }
            ExprKind::Match { scrutinee, arms } => {
                self.match_expr(expr.span, scrutinee, arms, expected)
            }
        }
    }

    /// The type an integer literal takes: the expected integer type, or
    /// `Int`.
    fn literal_type(&self, expected: Option<&Ty>) -> Ty {
        match expected.map(|e| self.infer.shallow(e)) {
            Some(ty) if ty.is_integer() => ty,
            _ => Ty::int(),
        }
    }

    /// Report an integer literal (negated when `negative`) that does not
    /// fit `ty`.
    fn range_check(&mut self, text: &str, negative: bool, ty: &Ty, span: Span) {
        let Some((min, max)) = ty.integer_range() else {
            return;
        };
        let digits: String = text.chars().filter(|&c| c != '_').collect();
        let fits = digits.parse::<i128>().ok().is_some_and(|value| {
            let value = if negative { -value } else { value };
            (min..=max).contains(&value)
        });
        if !fits {
            self.error(
                codes::LITERAL_OUT_OF_RANGE,
                format!("this literal does not fit in `{ty}`, which ranges from {min} to {max}"),
                span,
            );
        }
    }

    /// An interpolation hole, `{name}` or `{name.field}`.
    fn hole(&mut self, path: &[Ident]) {
        let Some((first, rest)) = path.split_first() else {
            return;
        };
        let mut ty = self.name(first);
        for field in rest {
            ty = self.field_of(ty, field);
        }
    }

    /// A name in value position.
    fn name(&mut self, ident: &Ident) -> Ty {
        let name = ident.name.as_str();
        if let Some(index) = self.locals.iter().rposition(|l| l.name == name) {
            let ty = self.locals[index].ty.clone();
            for frame in &mut self.closures {
                if index < frame.height {
                    frame.captures.push((name.to_string(), ty.clone()));
                }
            }
            return ty;
        }
        if let Some(sig) = self.items.fns.get(name).cloned() {
            let callable = self.instantiate(&sig, None);
            return callable.into_fn_type();
        }
        if let Some(enum_name) = self.items.bare_variants.get(name).cloned() {
            return self.variant_value(&enum_name, name);
        }
        if self.decls().get(name).is_some() {
            self.error(
                codes::NOT_A_VALUE,
                format!("`{name}` is a type, not a value"),
                ident.span,
            );
            return Ty::Error;
        }
        self.unknown_value(ident);
        Ty::Error
    }

    fn unknown_value(&mut self, ident: &Ident) {
        let mut candidates: Vec<&str> = self.locals.iter().map(|l| l.name.as_str()).collect();
        candidates.extend(self.items.fns.keys().map(String::as_str));
        candidates.extend(self.items.bare_variants.keys().map(String::as_str));
        let name = &ident.name;
        let diagnostic = suggest::unknown(
            codes::UNKNOWN_VALUE,
            format!("unknown name `{name}`"),
            name,
            ident.span,
            &candidates,
        );
        self.diagnostics.push(diagnostic);
    }

    /// `base.name` outside a call: a field, or a member of a type.
    fn field_expr(&mut self, base: &Expr, name: &Ident) -> Ty {
        if let Some(type_name) = self.type_name(base) {
            return match self.type_member(&type_name, name) {
                Some(callable) if callable.labels != Labels::Optional => {
                    if callable.params.is_empty() {
                        callable.ret
                    } else {
                        callable.into_fn_type()
                    }
                }
                Some(callable) => callable.into_fn_type(),
                None => Ty::Error,
            };
        }
        let ty = self.infer(base);
        self.field_of(ty, name)
    }

    /// The type of field `name` of a value of type `ty`.
    fn field_of(&mut self, ty: Ty, name: &Ident) -> Ty {
        let ty = self.infer.resolve(&ty);
        let type_name = match &ty {
            Ty::Error => return Ty::Error,
            Ty::Var(_) => {
                self.error(
                    codes::CANNOT_INFER,
                    format!("the type must be known before `.{}`", name.name),
                    name.span,
                );
                return Ty::Error;
            }
            Ty::Named(type_name, args, _) => {
                if let Some(info) = self.items.structs.get(type_name) {
                    if let Some(field_ty) = info.fields.field(&name.name) {
                        if info.hidden_fields.contains(&name.name) {
                            self.error(
                                codes::PRIVATE_FIELD,
                                format!("the field `{}` of `{type_name}` is private", name.name),
                                name.span,
                            );
                            return Ty::Error;
                        }
                        let subst: Vec<(String, Ty)> =
                            info.generics.iter().cloned().zip(args.clone()).collect();
                        return field_ty.substitute(&subst);
                    }
                }
                Some(type_name.clone())
            }
            _ => None,
        };
        let is_method = type_name
            .as_deref()
            .is_some_and(|t| self.items.method(t, &name.name).is_some());
        if is_method {
            self.error(
                codes::NO_FIELD,
                format!("`{}` is a method; call it with `()`", name.name),
                name.span,
            );
            return Ty::Error;
        }
        let fields: Vec<&str> = type_name
            .as_deref()
            .and_then(|t| self.items.structs.get(t))
            .map(|info| info.fields.names())
            .unwrap_or_default();
        let message = format!("`{ty}` has no field `{}`", name.name);
        self.report_with_suggestion(codes::NO_FIELD, message, name, &fields);
        Ty::Error
    }

    /// Report `message` at `ident`, with a fix replacing it by the closest
    /// of `candidates` when there is one.
    fn report_with_suggestion(
        &mut self,
        code: &'static str,
        message: String,
        ident: &Ident,
        candidates: &[&str],
    ) {
        self.diagnostics.push(suggest::unknown(
            code,
            message,
            &ident.name,
            ident.span,
            candidates,
        ));
    }

    // ---- Mutation ---------------------------------------------------------

    /// Report `place` when its root binding is not mutable. `action`
    /// completes "cannot ... `name`".
    fn require_mutable(&mut self, place: &Expr, code: &'static str, action: &str) {
        self.require_mutable_root(place, place.span, code, action);
    }

    /// Find `place`'s root binding, reporting at `span`, the whole place.
    fn require_mutable_root(&mut self, place: &Expr, span: Span, code: &'static str, action: &str) {
        match &place.kind {
            ExprKind::Field { base, .. } => self.require_mutable_root(base, span, code, action),
            ExprKind::Name(ident) => {
                let Some(local) = self.local(&ident.name) else {
                    return;
                };
                let name = &ident.name;
                let diagnostic = match local.binding {
                    Binding::Var => return,
                    Binding::Let(keyword) => Diagnostic::new(
                        code,
                        format!("cannot {action} `{name}`, which is declared with `let`"),
                        span,
                    )
                    .with_fix(
                        format!("declare `{name}` with `var`"),
                        vec![Edit {
                            span: keyword,
                            replacement: "var".to_string(),
                        }],
                    ),
                    Binding::Param | Binding::Loop | Binding::Pattern => Diagnostic::new(
                        code,
                        format!("cannot {action} `{name}`, which is not a `var` binding"),
                        span,
                    ),
                };
                self.diagnostics.push(diagnostic);
            }
            ExprKind::SelfValue => {
                let Some(receiver) = &self.receiver else {
                    return;
                };
                if receiver.mutable {
                    return;
                }
                let at = receiver.span.start;
                self.diagnostics.push(
                    Diagnostic::new(
                        code,
                        format!("cannot {action} `self` in a method that takes `self`"),
                        span,
                    )
                    .with_fix(
                        "make this a mutating method with `var self`",
                        vec![Edit::insert(at, "var ")],
                    ),
                );
            }
            // A temporary, such as a call's result, can be mutated freely.
            _ => {}
        }
    }

    // ---- Operators --------------------------------------------------------

    fn unary(&mut self, expr: &Expr, op: UnaryOp, operand: &Expr, expected: Option<&Ty>) -> Ty {
        match op {
            UnaryOp::Not => self.check(operand, &Ty::bool()),
            UnaryOp::Neg => {
                if let ExprKind::Int(text) = &operand.kind {
                    let ty = self.literal_type(expected);
                    if ty.is_signed_integer() {
                        self.range_check(text, true, &ty, expr.span);
                        return ty;
                    }
                }
                let ty = self.expr(operand, expected);
                let ty = self.infer.resolve(&ty);
                if ty.is_error() || ty.is_signed_integer() {
                    return ty;
                }
                self.error(
                    codes::BAD_OPERAND,
                    format!("unary `-` is not defined for `{ty}`"),
                    expr.span,
                );
                Ty::Error
            }
        }
    }

    fn binary(
        &mut self,
        expr: &Expr,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Ty>,
    ) -> Ty {
        match op {
            BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => {
                self.check(lhs, &Ty::bool());
                self.check(rhs, &Ty::bool());
                Ty::bool()
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                let ty = self.operands(lhs, rhs, None);
                self.require_trait(&ty, ty::EQ, &[], expr.span, &format!("`{}`", op.text()));
                Ty::bool()
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                self.numeric_operands(expr, op, lhs, rhs, None);
                Ty::bool()
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                self.numeric_operands(expr, op, lhs, rhs, expected)
            }
        }
    }

    /// Check two operands that must have the same type, and return it. A
    /// literal operand takes the other operand's type, so `1 + x` works
    /// for any integer `x`.
    fn operands(&mut self, lhs: &Expr, rhs: &Expr, expected: Option<&Ty>) -> Ty {
        let (first, second) = literal_last(lhs, rhs);
        let ty = self.expr(first, expected);
        let ty = self.infer.resolve(&ty);
        if ty.is_error() {
            self.infer(second);
            return Ty::Error;
        }
        self.check(second, &ty)
    }

    /// The operands of an arithmetic or comparison operator: numbers of
    /// one type. Returns that type, or [`Ty::Error`].
    fn numeric_operands(
        &mut self,
        expr: &Expr,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Ty>,
    ) -> Ty {
        let expected = expected.filter(|e| self.infer.shallow(e).is_integer());
        let (first, second) = literal_last(lhs, rhs);
        // Report a non-number before comparing the operands, so that
        // `s + 1` is one error about `+`.
        let ty = self.expr(first, expected);
        if !self.require_number(op, &ty, expr.span) {
            self.infer(second);
            return Ty::Error;
        }
        self.check(second, &ty);
        ty
    }

    /// Report `op` on a non-number. Returns whether `ty` is acceptable.
    fn require_number(&mut self, op: BinaryOp, ty: &Ty, span: Span) -> bool {
        let ty = self.infer.resolve(ty);
        if ty.is_error() || ty == Ty::Never || ty.is_integer() {
            return true;
        }
        if let Ty::Var(_) = ty {
            self.error(
                codes::CANNOT_INFER,
                format!("the operands' type must be known to use `{}`", op.text()),
                span,
            );
            return false;
        }
        let hint = if op == BinaryOp::Add && ty == Ty::string() {
            "; build strings with interpolation, as in \"{a}{b}\""
        } else {
            ""
        };
        self.error(
            codes::BAD_OPERAND,
            format!("`{}` is not defined for `{ty}`{hint}", op.text()),
            span,
        );
        false
    }

    // ---- Traits -----------------------------------------------------------

    /// Whether `ty` implements `trait_name<args>`: a generic parameter
    /// through its bounds, any other type through an `impl` (whose own
    /// bounds must hold in turn). A type not yet known, or an error,
    /// counts as implementing everything, so nothing is reported twice.
    fn implements(&self, ty: &Ty, trait_name: &str, args: &[Ty]) -> bool {
        let mut trial = self.infer.clone();
        self.implements_in(&mut trial, ty, trait_name, args, 0)
    }

    fn implements_in(
        &self,
        infer: &mut Infer,
        ty: &Ty,
        trait_name: &str,
        args: &[Ty],
        depth: usize,
    ) -> bool {
        // Bounds on a generic impl can recurse; give up generously.
        if depth > 16 {
            return true;
        }
        match infer.resolve(ty) {
            Ty::Error | Ty::Never | Ty::Var(_) => true,
            Ty::Unit => trait_name == ty::EQ,
            Ty::Fn { .. } => false,
            Ty::Param(p) => self.lower.bounds.iter().any(|b| {
                b.param == p
                    && b.trait_name == trait_name
                    && b.args.len() == args.len()
                    && b.args.iter().zip(args).all(|(x, y)| infer.unify(x, y))
            }),
            ty @ Ty::Named(..) => self.items.impls.iter().any(|imp| {
                if imp.trait_name != trait_name || imp.trait_args.len() != args.len() {
                    return false;
                }
                let mut attempt = infer.clone();
                let subst: Vec<(String, Ty)> = imp
                    .generics
                    .iter()
                    .map(|g| (g.clone(), attempt.fresh()))
                    .collect();
                let matches = attempt.unify(&ty, &imp.target.substitute(&subst))
                    && imp
                        .trait_args
                        .iter()
                        .zip(args)
                        .all(|(x, y)| attempt.unify(&x.substitute(&subst), y));
                let holds = matches
                    && imp.bounds.iter().all(|b| {
                        let param = Ty::Param(b.param.clone()).substitute(&subst);
                        let args: Vec<Ty> = b.args.iter().map(|a| a.substitute(&subst)).collect();
                        self.implements_in(&mut attempt, &param, &b.trait_name, &args, depth + 1)
                    });
                if holds {
                    *infer = attempt;
                }
                holds
            }),
        }
    }

    /// Report `E0265` at `span` when `ty` does not implement
    /// `trait_name<args>`, which `what` needs. On a generic parameter of
    /// this function, the fix adds the bound.
    fn require_trait(&mut self, ty: &Ty, trait_name: &str, args: &[Ty], span: Span, what: &str) {
        if self.implements(ty, trait_name, args) {
            return;
        }
        let ty = self.infer.resolve(ty);
        let bound = Ty::generic(trait_name, args.to_vec()).to_string();
        let message =
            format!("{what} needs `{ty}: {bound}`, but `{ty}` does not implement `{bound}`");
        let mut diagnostic = Diagnostic::new(codes::NOT_IMPLEMENTED, message, span);
        if let Ty::Param(p) = &ty {
            if let Some(site) = self.lower.sites.iter().rev().find(|s| &s.name == p) {
                let text = if site.has_bounds {
                    format!(" + {bound}")
                } else {
                    format!(": {bound}")
                };
                diagnostic = diagnostic.with_fix(
                    format!("add the bound `{p}: {bound}`"),
                    vec![Edit::insert(site.end, text)],
                );
            }
        }
        self.diagnostics.push(diagnostic);
    }

    // ---- Closures -------------------------------------------------------

    /// `|params| body`. Parameter types come from annotations or the
    /// expected function type.
    pub fn closure(
        &mut self,
        params: &[ClosureParam],
        body: &Expr,
        expected: Option<&Ty>,
        span: Span,
    ) -> Ty {
        let expected_fn = expected.cloned();
        let expected = match expected.map(|e| self.infer.resolve(e)) {
            Some(Ty::Fn {
                params: expected_params,
                ret,
                ..
            }) if expected_params.len() == params.len() => Some((expected_params, *ret)),
            _ => None,
        };
        let mut param_tys = Vec::new();
        for (i, param) in params.iter().enumerate() {
            let hint = expected.as_ref().map(|(ps, _)| ps[i].clone());
            let ty = match (&param.ty, hint) {
                (Some(written), hint) => {
                    let ty = self.lower.ty(written);
                    match hint {
                        // The caller passes a value of the expected
                        // parameter type into the written one.
                        Some(hint) => self.expect(&hint, &ty, written.span),
                        None => ty,
                    }
                }
                (None, Some(hint)) => hint,
                (None, None) => self.infer.fresh(),
            };
            param_tys.push(ty);
        }
        let ret = match &expected {
            Some((_, ret)) => ret.clone(),
            None => self.infer.fresh(),
        };
        let height = self.locals.len();
        self.closures.push(ClosureFrame {
            height,
            captures: Vec::new(),
        });
        for (param, ty) in params.iter().zip(&param_tys) {
            self.bind(&param.name.name, ty.clone(), Binding::Param);
        }
        let outer_ret = std::mem::replace(&mut self.ret, ret.clone());
        let outer_loops = std::mem::take(&mut self.loop_depth);
        self.check(body, &ret);
        self.ret = outer_ret;
        self.loop_depth = outer_loops;
        self.locals.truncate(height);
        let frame = self.closures.pop().expect("the closure's own frame");
        let captured = frame
            .captures
            .into_iter()
            .find(|(_, ty)| self.bearing(ty))
            .map(|(name, _)| name);
        let ty = Ty::Fn {
            cap: captured.is_some(),
            params: param_tys,
            ret: Box::new(ret),
        };
        let wants_plain_fn = matches!(
            expected_fn.map(|e| self.infer.shallow(&e)),
            Some(Ty::Fn { cap: false, .. })
        );
        match captured {
            Some(name) if wants_plain_fn => {
                self.error(
                    codes::CAPTURING_CLOSURE,
                    format!(
                        "this closure captures the capability-bearing `{name}`, so it is a \
                         `cap fn`, but a plain `fn` is expected here"
                    ),
                    span,
                );
                Ty::Error
            }
            _ => ty,
        }
    }

    /// Whether `ty`, as solved so far, is capability-bearing.
    fn bearing(&self, ty: &Ty) -> bool {
        capability_bearing(&self.infer.resolve(ty), self.decls())
    }

    // ---- Blocks and statements --------------------------------------------

    /// A block's type is its final expression's, `()` without one, or
    /// [`Ty::Never`] when it ends by leaving (`return`, `break`,
    /// `continue`). With `use_value` false, the final expression's value
    /// is discarded.
    fn block(&mut self, block: &Block, expected: Option<&Ty>, use_value: bool) -> Ty {
        let height = self.locals.len();
        let mut ty = Ty::Unit;
        for (i, stmt) in block.stmts.iter().enumerate() {
            let last = i + 1 == block.stmts.len();
            match &stmt.kind {
                StmtKind::Expr(expr) if last && use_value => {
                    ty = match expected {
                        Some(expected) => self.check(expr, expected),
                        None => self.infer(expr),
                    };
                }
                StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue if last => {
                    self.stmt(stmt);
                    ty = Ty::Never;
                }
                _ => self.stmt(stmt),
            }
        }
        self.locals.truncate(height);
        let ends_with_value = matches!(
            block.stmts.last().map(|s| &s.kind),
            Some(StmtKind::Expr(_) | StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue)
        );
        match expected {
            Some(expected) if use_value && !ends_with_value => {
                self.expect(&ty, expected, block.span)
            }
            _ => ty,
        }
    }

    fn stmt(&mut self, stmt: &oska_syntax::ast::Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                self.require_lowercase(name);
                let ty = match ty {
                    Some(written) => {
                        let ty = self.lower.ty(written);
                        self.check(value, &ty);
                        ty
                    }
                    None => {
                        let ty = self.infer(value);
                        if name.name != "_" {
                            self.inferred_lets.push((name.clone(), ty.clone()));
                        }
                        ty
                    }
                };
                let binding = if *mutable {
                    Binding::Var
                } else {
                    let keyword = Span {
                        start: stmt.span.start,
                        end: stmt.span.start + "let".len(),
                    };
                    Binding::Let(keyword)
                };
                self.bind(&name.name, ty, binding);
            }
            StmtKind::Assign { target, value } => {
                let ty = self.infer(target);
                self.require_mutable(target, codes::ASSIGN_TO_LET, "assign to");
                self.check(value, &ty);
            }
            StmtKind::Return(value) => {
                let ret = self.ret.clone();
                match value {
                    Some(value) => {
                        self.check(value, &ret);
                    }
                    None => {
                        self.expect(&Ty::Unit, &ret, stmt.span);
                    }
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    let keyword = if matches!(stmt.kind, StmtKind::Break) {
                        "break"
                    } else {
                        "continue"
                    };
                    self.error(
                        codes::OUTSIDE_LOOP,
                        format!("`{keyword}` outside of a loop"),
                        stmt.span,
                    );
                }
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                let ty = self.infer(iter);
                let element = match self.infer.resolve(&ty) {
                    Ty::Named(name, args, _)
                        if (name == ty::LIST || name == ty::RANGE) && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    Ty::Error => Ty::Error,
                    other => {
                        self.error(
                            codes::NOT_ITERABLE,
                            format!("cannot iterate over `{other}`; expected a `List` or a range"),
                            iter.span,
                        );
                        Ty::Error
                    }
                };
                self.require_lowercase(binding);
                let height = self.locals.len();
                self.bind(&binding.name, element, Binding::Loop);
                self.loop_body(body);
                self.locals.truncate(height);
            }
            StmtKind::While { cond, body } => {
                self.check(cond, &Ty::bool());
                self.loop_body(body);
            }
            StmtKind::Expr(expr) => self.expr_stmt(expr),
        }
    }

    fn loop_body(&mut self, body: &Block) {
        self.loop_depth += 1;
        self.block(body, None, false);
        self.loop_depth -= 1;
    }
}

/// The operands in checking order: an integer literal goes second, so
/// it takes the other operand's type.
fn literal_last<'e>(lhs: &'e Expr, rhs: &'e Expr) -> (&'e Expr, &'e Expr) {
    if is_int_literal(lhs) && !is_int_literal(rhs) {
        (rhs, lhs)
    } else {
        (lhs, rhs)
    }
}

fn is_int_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => matches!(operand.kind, ExprKind::Int(_)),
        _ => false,
    }
}
