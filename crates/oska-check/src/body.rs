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
//! - **Statements:** [`Body::expr_stmt`] sees every expression whose value
//!   is discarded.
//! - **Patterns:** [`Body::bind_pattern`] binds a pattern's names; for now
//!   every binding has the error type.
//! - **`?`:** [`Body::try_expr`]. Error conversion through `impl From`
//!   goes in [`Body::converts_from`].
//! - **Method lookup:** [`Body::method_call`] and [`Items::method`].
//! - **Closures:** [`Body::closure`]. Locals are a stack, so a closure's
//!   captures are the names it resolves below the stack height at which
//!   the closure's parameters were pushed (plus `self`). Each closure
//!   being checked has a frame recording them; a closure that captures a
//!   capability-bearing value is a `cap fn`. Unification ignores `cap`,
//!   so [`Body::expect`] reports a `cap fn` used as a plain `fn`.

use oska_syntax::ast::{
    Arg, BinaryOp, Block, ClosureParam, Expr, ExprKind, FnDecl, Ident, ItemKind, MatchArm, Module,
    Pattern, PatternKind, StmtKind, StrPart, UnaryOp,
};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::codes;
use crate::decls::{DeclKind, Decls};
use crate::infer::Infer;
use crate::items::{FieldsInfo, FnSig, Items, Lower, Shape};
use crate::resolve::closest;
use crate::ty::Ty;

/// The prelude type of `t`, the stand-in capabilities in a `test` body.
const TEST_CONTEXT: &str = "TestContext";

/// Check every function, method, constant, and test body in `module`. A
/// test body sees `t`, and nothing else beyond pure code.
pub fn module(module: &Module, decls: &Decls, items: &Items, diagnostics: &mut Vec<Diagnostic>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => function(f, Lower::new(decls), items, diagnostics),
            ItemKind::Impl(imp) => {
                let lower = Lower::new(decls).with_generics(&imp.generics);
                let target = lower.ty(&imp.target);
                let lower = lower.with_self(target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        function(f, lower.clone(), items, diagnostics);
                    }
                }
            }
            ItemKind::Trait(t) => {
                let lower = Lower::new(decls)
                    .with_generics(&t.generics)
                    .with_self(Ty::Param("Self".to_string()));
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        function(f, lower.clone(), items, diagnostics);
                    }
                }
            }
            ItemKind::Const(c) => {
                let lower = Lower::new(decls);
                let ty = lower.ty(&c.ty);
                let mut body = Body::new(items, lower, Ty::Unit, diagnostics);
                body.check(&c.value, &ty);
            }
            ItemKind::Test(test) => {
                let mut body = Body::new(items, Lower::new(decls), Ty::Unit, diagnostics);
                body.bind("t", Ty::named(TEST_CONTEXT), Binding::Param);
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
fn function(f: &FnDecl, lower: Lower, items: &Items, diagnostics: &mut Vec<Diagnostic>) {
    if f.body.is_none() && f.requires.is_empty() && f.ensures.is_empty() {
        return;
    }
    let lower = lower.with_generics(&f.generics);
    let sig = lower.signature(f, &[], lower.self_ty.clone());
    let mut body = Body::new(items, lower, sig.ret.clone(), diagnostics);
    if let Some(receiver) = &f.receiver {
        body.receiver = Some(Receiver {
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
struct Receiver {
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
    /// A pure function: no parameter, the receiver included, is
    /// capability-bearing. Constructors are pure; a closure is pure
    /// unless it is a `cap fn`.
    pure: bool,
}

/// The state of checking one body.
pub struct Body<'c, 'd, 'a> {
    items: &'c Items,
    lower: Lower<'d, 'a>,
    infer: Infer,
    diagnostics: &'c mut Vec<Diagnostic>,
    /// Locals in scope, innermost last. Shadowing pushes a new one.
    locals: Vec<Local>,
    receiver: Option<Receiver>,
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
        items: &'c Items,
        lower: Lower<'d, 'a>,
        ret: Ty,
        diagnostics: &'c mut Vec<Diagnostic>,
    ) -> Self {
        Body {
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
        if self.infer.unify(actual, expected) {
            // Unification ignores `cap`, so a `cap fn` hidden where a plain
            // `fn` is expected is caught here.
            let actual_resolved = self.infer.resolve(actual);
            let expected_resolved = self.infer.resolve(expected);
            if cap_lost(&actual_resolved, &expected_resolved, true) {
                self.error(
                    codes::CAPTURING_CLOSURE,
                    format!(
                        "expected `{expected_resolved}`, found `{actual_resolved}`: a `cap fn` \
                         can carry authority, so it cannot be used as a plain `fn`"
                    ),
                    span,
                );
                return Ty::Error;
            }
            return actual.clone();
        }
        let expected = self.infer.resolve(expected);
        let actual = self.infer.resolve(actual);
        self.error(
            codes::MISMATCH,
            format!("expected `{expected}`, found `{actual}`"),
            span,
        );
        Ty::Error
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
                Ty::Named("Range".to_string(), vec![ty])
            }
            ExprKind::List(elements) => {
                let element = match expected.map(|e| self.infer.shallow(e)) {
                    Some(Ty::Named(name, args)) if name == "List" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => self.infer.fresh(),
                };
                for e in elements {
                    self.check(e, &element);
                }
                Ty::Named("List".to_string(), vec![element])
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
                    ty: Ty::named("Scope"),
                    binding: Binding::Pattern,
                });
                let ty = self.block(body, expected, true);
                self.locals.pop();
                ty
            }
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, expected),
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
        let diagnostic = match closest(name, &candidates) {
            Some(suggestion) => Diagnostic::new(
                codes::UNKNOWN_VALUE,
                format!("unknown name `{name}`; did you mean `{suggestion}`?"),
                ident.span,
            )
            .with_fix(
                format!("replace with `{suggestion}`"),
                vec![Edit {
                    span: ident.span,
                    replacement: suggestion.to_string(),
                }],
            ),
            None => Diagnostic::new(
                codes::UNKNOWN_VALUE,
                format!("unknown name `{name}`"),
                ident.span,
            ),
        };
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
            Ty::Named(type_name, args) => {
                if let Some(info) = self.items.structs.get(type_name) {
                    let field = info
                        .fields
                        .fields
                        .iter()
                        .find(|(n, _)| n.as_deref() == Some(name.name.as_str()));
                    if let Some((_, field_ty)) = field {
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
            .map(|info| {
                info.fields
                    .fields
                    .iter()
                    .filter_map(|(n, _)| n.as_deref())
                    .collect()
            })
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
        let diagnostic = match closest(&ident.name, candidates) {
            Some(suggestion) => Diagnostic::new(
                code,
                format!("{message}; did you mean `{suggestion}`?"),
                ident.span,
            )
            .with_fix(
                format!("replace with `{suggestion}`"),
                vec![Edit {
                    span: ident.span,
                    replacement: suggestion.to_string(),
                }],
            ),
            None => Diagnostic::new(code, message, ident.span),
        };
        self.diagnostics.push(diagnostic);
    }

    // ---- Calls and construction -------------------------------------------

    fn call(&mut self, expr: &Expr, callee: &Expr, args: &[Arg], expected: Option<&Ty>) -> Ty {
        let callable = match &callee.kind {
            ExprKind::Field { base, name } => match self.type_name(base) {
                Some(type_name) => self.type_member(&type_name, name),
                None => return self.method_call(base, name, args, expected, expr.span),
            },
            ExprKind::Name(ident) if self.local(&ident.name).is_none() => self.named_callee(ident),
            _ => {
                let ty = self.infer(callee);
                match self.infer.resolve(&ty) {
                    Ty::Fn { cap, params, ret } => Some(Callable {
                        description: "this function".to_string(),
                        params: params.into_iter().map(|p| (None, p)).collect(),
                        ret: *ret,
                        labels: Labels::Optional,
                        pure: !cap,
                    }),
                    Ty::Error => None,
                    other => {
                        self.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not a function"),
                            callee.span,
                        );
                        None
                    }
                }
            }
        };
        match callable {
            Some(callable) => self.apply(callable, args, expected, expr.span),
            None => {
                self.infer_args(args);
                Ty::Error
            }
        }
    }

    /// A bare name called: a function, a struct, or a prelude variant.
    fn named_callee(&mut self, ident: &Ident) -> Option<Callable> {
        let name = ident.name.as_str();
        if let Some(sig) = self.items.fns.get(name).cloned() {
            return Some(self.instantiate(&sig, None));
        }
        if let Some(info) = self.items.structs.get(name).cloned() {
            if info.sealed {
                let capability =
                    matches!(self.decls().get(name), Some(d) if d.kind == DeclKind::Capability);
                let message = if capability {
                    format!(
                        "`{name}` is a built-in capability and cannot be constructed; \
                         take it as a parameter, ultimately from `main`"
                    )
                } else {
                    format!(
                        "`{name}` has private fields and cannot be constructed outside the prelude"
                    )
                };
                self.error(codes::FORGED, message, ident.span);
                return None;
            }
            return Some(self.constructor(name, &info.generics, &info.fields, name.to_string()));
        }
        if let Some(enum_name) = self.items.bare_variants.get(name).cloned() {
            return self.variant(&enum_name, name);
        }
        if self.decls().get(name).is_some() {
            self.error(
                codes::NOT_A_VALUE,
                format!("`{name}` cannot be constructed"),
                ident.span,
            );
            return None;
        }
        self.unknown_value(ident);
        None
    }

    /// `Type.name`: a variant of an enum, or an associated function or
    /// method of the type. A method called this way takes its receiver as
    /// the first argument.
    fn type_member(&mut self, type_name: &str, name: &Ident) -> Option<Callable> {
        if self.items.enums.contains_key(type_name) {
            if let Some(callable) = self.variant(type_name, &name.name) {
                return Some(callable);
            }
        }
        if let Some(sig) = self.items.method(type_name, &name.name).cloned() {
            let mut callable = self.instantiate(&sig, None);
            if sig.receiver.is_some() {
                let self_ty = sig.self_ty.clone().unwrap_or(Ty::Error);
                callable.params.insert(0, (None, self_ty));
            }
            return Some(callable);
        }
        if matches!(self.decls().get(type_name), Some(d) if d.kind == DeclKind::Trait) {
            // Qualified trait calls arrive with generics and traits.
            return None;
        }
        let mut candidates = self.items.method_names(type_name);
        if let Some(info) = self.items.enums.get(type_name) {
            candidates.extend(info.variants.iter().map(|(v, _)| v.as_str()));
        }
        let message = format!("`{type_name}` has no variant or function `{}`", name.name);
        self.report_with_suggestion(codes::NO_MEMBER, message, name, &candidates);
        None
    }

    /// The constructor of `variant` of `enum_name`, instantiated.
    fn variant(&mut self, enum_name: &str, variant: &str) -> Option<Callable> {
        let info = self.items.enums.get(enum_name)?.clone();
        let (_, fields) = info.variants.iter().find(|(v, _)| v == variant)?;
        Some(self.constructor(
            enum_name,
            &info.generics,
            fields,
            format!("{enum_name}.{variant}"),
        ))
    }

    /// A variant named without a call: a value when it is fieldless, and
    /// its constructor function otherwise.
    fn variant_value(&mut self, enum_name: &str, variant: &str) -> Ty {
        match self.variant(enum_name, variant) {
            Some(callable) if callable.params.is_empty() => callable.ret,
            Some(callable) => callable.into_fn_type(),
            None => Ty::Error,
        }
    }

    fn constructor(
        &mut self,
        type_name: &str,
        generics: &[String],
        fields: &FieldsInfo,
        description: String,
    ) -> Callable {
        let subst: Vec<(String, Ty)> = generics
            .iter()
            .map(|g| (g.clone(), self.infer.fresh()))
            .collect();
        let ret = Ty::Named(
            type_name.to_string(),
            subst.iter().map(|(_, t)| t.clone()).collect(),
        );
        Callable {
            description,
            params: fields
                .fields
                .iter()
                .map(|(n, t)| (n.clone(), t.substitute(&subst)))
                .collect(),
            ret,
            labels: match fields.shape {
                Shape::Named => Labels::Required,
                Shape::Positional | Shape::Unit => Labels::None,
            },
            pure: true,
        }
    }

    /// Instantiate `sig`'s generics with fresh variables. `self_ty`, when
    /// given, is unified with the signature's `Self` type, which solves an
    /// impl's generics from the receiver.
    fn instantiate(&mut self, sig: &FnSig, self_ty: Option<&Ty>) -> Callable {
        let subst: Vec<(String, Ty)> = sig
            .generics
            .iter()
            .map(|g| (g.clone(), self.infer.fresh()))
            .collect();
        if let (Some(actual), Some(declared)) = (self_ty, &sig.self_ty) {
            let declared = declared.substitute(&subst);
            self.infer.unify(&declared, actual);
        }
        Callable {
            description: format!("`{}`", sig.name),
            params: sig
                .params
                .iter()
                .map(|(n, t)| (Some(n.clone()), t.substitute(&subst)))
                .collect(),
            ret: sig.ret.substitute(&subst),
            labels: Labels::Optional,
            pure: sig.pure,
        }
    }

    /// `base.name(args)`: a method call.
    fn method_call(
        &mut self,
        base: &Expr,
        name: &Ident,
        args: &[Arg],
        expected: Option<&Ty>,
        span: Span,
    ) -> Ty {
        let receiver = self.infer(base);
        let receiver = self.infer.resolve(&receiver);
        let type_name = match &receiver {
            Ty::Named(type_name, _) => type_name.clone(),
            Ty::Var(_) => {
                self.error(
                    codes::CANNOT_INFER,
                    format!("the type must be known before `.{}`", name.name),
                    name.span,
                );
                self.infer_args(args);
                return Ty::Error;
            }
            // Methods on generic parameters come from their bounds, which
            // arrive with generics and traits.
            Ty::Error | Ty::Param(_) | Ty::Never => {
                self.infer_args(args);
                return Ty::Error;
            }
            Ty::Unit | Ty::Fn { .. } => receiver.to_string(),
        };
        let Some(sig) = self.items.method(&type_name, &name.name).cloned() else {
            let candidates = self.items.method_names(&type_name);
            let message = format!("`{receiver}` has no method `{}`", name.name);
            self.report_with_suggestion(codes::NO_MEMBER, message, name, &candidates);
            self.infer_args(args);
            return Ty::Error;
        };
        let Some(mutating) = sig.receiver else {
            self.error(
                codes::NO_MEMBER,
                format!(
                    "`{}` is an associated function; call it as `{type_name}.{}(...)`",
                    name.name, name.name
                ),
                name.span,
            );
            self.infer_args(args);
            return Ty::Error;
        };
        if mutating {
            self.require_mutable(
                base,
                codes::MUTATING_CALL_ON_LET,
                &format!("call the mutating method `{}` on", name.name),
            );
        }
        let callable = self.instantiate(&sig, Some(&receiver));
        self.apply(callable, args, expected, span)
    }

    /// Check `args` against `callable`'s parameters and return its result.
    fn apply(&mut self, callable: Callable, args: &[Arg], expected: Option<&Ty>, span: Span) -> Ty {
        // Let the expected result type guide the arguments first, as in
        // `let r: Result<U8, E> = Ok(200)`. A mismatch is reported later,
        // against the whole call.
        if let Some(expected) = expected {
            self.infer.unify(&callable.ret, expected);
        }
        if self.in_contract && !callable.pure {
            self.error(
                codes::IMPURE_IN_CONTRACT,
                format!(
                    "a contract may call only pure functions, but {} takes a capability",
                    callable.description
                ),
                span,
            );
        }
        if args.len() != callable.params.len() {
            let n = callable.params.len();
            self.error(
                codes::ARGUMENT_COUNT,
                format!(
                    "{} takes {n} argument{}, but {} {} given",
                    callable.description,
                    if n == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" },
                ),
                span,
            );
        }
        for (i, arg) in args.iter().enumerate() {
            match callable.params.get(i) {
                Some((param, ty)) => {
                    self.label(arg, param.as_deref(), callable.labels);
                    self.check(&arg.value, ty);
                }
                None => {
                    self.infer(&arg.value);
                }
            }
        }
        callable.ret
    }

    /// Check an argument's label against its parameter.
    fn label(&mut self, arg: &Arg, param: Option<&str>, labels: Labels) {
        match (&arg.label, param, labels) {
            (Some(label), Some(param), _) if label.name != param => {
                self.diagnostics.push(
                    Diagnostic::new(
                        codes::BAD_LABEL,
                        format!("expected the label `{param}`, found `{}`", label.name),
                        label.span,
                    )
                    .with_fix(
                        format!("replace with `{param}`"),
                        vec![Edit {
                            span: label.span,
                            replacement: param.to_string(),
                        }],
                    ),
                );
            }
            (Some(label), None, _) => {
                let span = Span {
                    start: label.span.start,
                    end: arg.value.span.start,
                };
                self.diagnostics.push(
                    Diagnostic::new(
                        codes::BAD_LABEL,
                        format!("positional arguments take no label, found `{}`", label.name),
                        label.span,
                    )
                    .with_fix("remove the label", vec![Edit::delete(span)]),
                );
            }
            (None, Some(param), Labels::Required) => {
                self.diagnostics.push(
                    Diagnostic::new(
                        codes::MISSING_LABEL,
                        format!("named fields are constructed by name: write `{param}: ...`"),
                        arg.value.span,
                    )
                    .with_fix(
                        format!("label the argument `{param}`"),
                        vec![Edit::insert(arg.value.span.start, format!("{param}: "))],
                    ),
                );
            }
            _ => {}
        }
    }

    fn infer_args(&mut self, args: &[Arg]) {
        for arg in args {
            self.infer(&arg.value);
        }
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
                self.operands(lhs, rhs, None);
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

    // ---- Closures, `?`, and `match` ---------------------------------------

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
                    if let Some(hint) = hint {
                        self.expect(&ty, &hint, written.span);
                    }
                    ty
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

    /// Whether `ty` is **capability-bearing**: it contains a built-in
    /// capability or a `cap fn`, through type arguments and the fields of
    /// structs and enums.
    fn bearing(&self, ty: &Ty) -> bool {
        let ty = self.infer.resolve(ty);
        bearing(&ty, self.items, self.decls(), &mut Vec::new())
    }

    /// `operand?` (the whole expression is `expr`): the success value of a
    /// `Result` or an `Option`, whose failure the enclosing function (or
    /// closure) returns. That function must return the same kind: `?`
    /// never crosses between `Option` and `Result` (`E0243`). A `Result`'s
    /// error type goes through [`Body::propagate_error`].
    pub fn try_expr(&mut self, expr: &Expr, operand: &Expr) -> Ty {
        let ty = self.infer(operand);
        let (kind, args) = match self.infer.resolve(&ty) {
            Ty::Named(name, args)
                if (name == "Result" && args.len() == 2)
                    || (name == "Option" && args.len() == 1) =>
            {
                (name, args)
            }
            // Unknown or already reported.
            Ty::Error | Ty::Var(_) | Ty::Never => return Ty::Error,
            other => {
                self.error(
                    codes::TRY_OPERAND,
                    format!("`?` needs a `Result` or an `Option`, found `{other}`"),
                    expr.span,
                );
                return Ty::Error;
            }
        };
        let ret = self.ret.clone();
        match self.infer.resolve(&ret) {
            Ty::Error => {}
            // A closure whose return type is not known yet returns the
            // same kind, with its own success type.
            Ty::Var(_) => {
                let failure = if kind == "Result" {
                    Ty::Named(kind.clone(), vec![self.infer.fresh(), args[1].clone()])
                } else {
                    Ty::Named(kind.clone(), vec![self.infer.fresh()])
                };
                self.infer.unify(&ret, &failure);
            }
            Ty::Named(name, ret_args) if name == kind => {
                if kind == "Result" && ret_args.len() == 2 {
                    self.propagate_error(&args[1], &ret_args[1], expr.span);
                }
            }
            other => {
                let advice = match (kind.as_str(), other.head()) {
                    ("Option", Some("Result")) => {
                        "; convert it explicitly, for example with `.ok_or(err)?`"
                    }
                    ("Result", Some("Option")) => {
                        "; convert it explicitly, for example with a `match`"
                    }
                    _ => "",
                };
                self.error(
                    codes::TRY_KIND,
                    format!(
                        "`?` on an `{kind}` must be in a function that returns `{kind}`, \
                         but this one returns `{other}`{advice}"
                    ),
                    expr.span,
                );
            }
        }
        args[0].clone()
    }

    /// `?` propagates a `Result`'s error of type `from` out of a function
    /// whose error type is `to`. They must be the same type, or `to` must
    /// convert from `from` (`E0242`).
    fn propagate_error(&mut self, from: &Ty, to: &Ty, span: Span) {
        if self.infer.unify(from, to) {
            return;
        }
        let from = self.infer.resolve(from);
        let to = self.infer.resolve(to);
        if from.is_error() || to.is_error() || self.converts_from(&from, &to) {
            return;
        }
        self.error(
            codes::TRY_ERROR_TYPE,
            format!(
                "`?` cannot propagate an error of type `{from}` from a function whose \
                 error type is `{to}`; write `impl From<{from}> for {to}` or convert it explicitly"
            ),
            span,
        );
    }

    /// Whether `?` can convert an error of type `from` into `to`.
    ///
    /// Extension point for `impl From` conversion (#39), which should look
    /// up `impl From<from> for to` through the trait machinery. Until then,
    /// this accepts a trait-impl method `from(value: from)` on `to`, so
    /// that `examples/capabilities.osk` keeps checking.
    fn converts_from(&self, from: &Ty, to: &Ty) -> bool {
        let Some(type_name) = to.head() else {
            return false;
        };
        self.items
            .trait_impls
            .get(type_name)
            .into_iter()
            .flatten()
            .any(|sig| {
                sig.name == "from"
                    && sig.receiver.is_none()
                    && matches!(sig.params.as_slice(), [(_, param)] if param == from)
            })
    }

    /// `match`. Arm bodies must agree; patterns are only bound, not yet
    /// checked against the scrutinee.
    fn match_expr(&mut self, scrutinee: &Expr, arms: &[MatchArm], expected: Option<&Ty>) -> Ty {
        self.infer(scrutinee);
        let mut result: Option<Ty> = expected.cloned();
        for arm in arms {
            let height = self.locals.len();
            self.bind_pattern(&arm.pattern);
            if let Some(guard) = &arm.guard {
                self.check(guard, &Ty::bool());
            }
            match &result {
                Some(ty) => {
                    let ty = ty.clone();
                    self.check(&arm.body, &ty);
                }
                None => {
                    let ty = self.infer(&arm.body);
                    if self.infer.shallow(&ty) != Ty::Never {
                        result = Some(ty);
                    }
                }
            }
            self.locals.truncate(height);
        }
        result.unwrap_or(Ty::Never)
    }

    /// Bind the names a pattern introduces. Each has the error type until
    /// patterns are checked against the scrutinee.
    pub fn bind_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::Binding(ident) => {
                self.bind(&ident.name, Ty::Error, Binding::Pattern);
            }
            PatternKind::Constructor { args, .. } => {
                for arg in args {
                    self.bind_pattern(&arg.pattern);
                }
            }
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
        }
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
                    Ty::Named(name, args)
                        if (name == "List" || name == "Range") && args.len() == 1 =>
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

    /// An expression whose value is discarded. Dropping a `Result` is an
    /// error; `let _ = f()` discards one deliberately.
    pub fn expr_stmt(&mut self, expr: &Expr) {
        let ty = self.infer(expr);
        if let Ty::Named(name, _) = self.infer.resolve(&ty) {
            if name == "Result" {
                self.error(
                    codes::IGNORED_RESULT,
                    "this `Result` is ignored; handle it, propagate it with `?`, \
                     or discard it with `let _ = ...`",
                    expr.span,
                );
            }
        }
    }
}

impl Callable {
    /// The function type of a callable used as a value.
    fn into_fn_type(self) -> Ty {
        Ty::Fn {
            cap: false,
            params: self.params.into_iter().map(|(_, t)| t).collect(),
            ret: Box::new(self.ret),
        }
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

/// Whether using a value of type `actual` as `expected` (types that
/// unify) would pass a `cap fn` off as a plain `fn`. `positive` is false
/// inside a function type's parameters, where the direction flips.
fn cap_lost(actual: &Ty, expected: &Ty, positive: bool) -> bool {
    match (actual, expected) {
        (
            Ty::Fn {
                cap: actual_cap,
                params: actual_params,
                ret: actual_ret,
            },
            Ty::Fn {
                cap: expected_cap,
                params: expected_params,
                ret: expected_ret,
            },
        ) => {
            let lost = if positive {
                *actual_cap && !*expected_cap
            } else {
                *expected_cap && !*actual_cap
            };
            lost || cap_lost(actual_ret, expected_ret, positive)
                || actual_params
                    .iter()
                    .zip(expected_params)
                    .any(|(a, e)| cap_lost(a, e, !positive))
        }
        (Ty::Named(_, actual_args), Ty::Named(_, expected_args)) => actual_args
            .iter()
            .zip(expected_args)
            .any(|(a, e)| cap_lost(a, e, positive)),
        _ => false,
    }
}

/// Whether the resolved type `ty` is capability-bearing. `expanding`
/// holds the types being expanded, so recursive types end.
fn bearing(ty: &Ty, items: &Items, decls: &Decls, expanding: &mut Vec<String>) -> bool {
    match ty {
        Ty::Fn { cap, .. } => *cap,
        Ty::Named(name, args) => {
            if matches!(decls.get(name), Some(d) if d.kind == DeclKind::Capability) {
                return true;
            }
            if args.iter().any(|a| bearing(a, items, decls, expanding)) {
                return true;
            }
            if expanding.contains(name) {
                return false;
            }
            expanding.push(name.clone());
            let (generics, fields): (&[String], Vec<&Ty>) =
                if let Some(info) = items.structs.get(name) {
                    (
                        &info.generics,
                        info.fields.fields.iter().map(|(_, t)| t).collect(),
                    )
                } else if let Some(info) = items.enums.get(name) {
                    let fields = info
                        .variants
                        .iter()
                        .flat_map(|(_, f)| f.fields.iter().map(|(_, t)| t))
                        .collect();
                    (&info.generics, fields)
                } else {
                    (&[], Vec::new())
                };
            let subst: Vec<(String, Ty)> = generics.iter().cloned().zip(args.clone()).collect();
            let found = fields
                .into_iter()
                .any(|f| bearing(&f.substitute(&subst), items, decls, expanding));
            expanding.pop();
            found
        }
        Ty::Unit | Ty::Param(_) | Ty::Var(_) | Ty::Never | Ty::Error => false,
    }
}
