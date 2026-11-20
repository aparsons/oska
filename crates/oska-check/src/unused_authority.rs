//! **Unused authority** (ADR 0016): under `--release`, a
//! capability-bearing parameter that its function never uses (`E0296`),
//! or a capability-bearing field of a `cap struct` that no code in the
//! program reads (`E0297`), is an error.
//!
//! Any read of the parameter in the body counts as a use: a method call
//! on it, passing it on, attenuating it, storing it, returning it, or
//! capturing it in a closure. The rule does not try to prove the
//! capability is reached, so it has no false positives. A read in a
//! `requires` or `ensures` clause counts too: a parameter of a bounded type
//! parameter may be read there (`E0274` keeps its type argument pure), and
//! reading a capability there is already `E0280`. Two reads are not uses,
//! since neither can exercise the capability:
//!
//! - a read of a name that a `let`, a `for`, a closure parameter, a match
//!   binding or a `scope` handle binds over the parameter (names are
//!   resolved by scope);
//! - `let _ = name`, which only discards it.
//!
//! What is capability-bearing is [`authority::bearing_params`], the
//! authority report's own definition. A method whose signature a trait
//! fixes is exempt: an `impl Trait for Type` method, and a trait's default
//! method. Naming a parameter `_name` does not silence the check. The
//! receiver is not a parameter that can be removed, so it is not judged.
//!
//! A field is read anywhere in the program by a field access, which also
//! covers passing it on and calling a method on it, or by a constructor
//! pattern that does not discard it with `_`; the body checker records
//! each read (`Findings::field_reads` in `body.rs`). A field is
//! capability-bearing as ADR 0002 says ([`authority::bearing_field`]): a
//! field typed by one of the struct's own type parameters is judged only
//! when that parameter has a bound. Field
//! reads are known only once every body has checked, so the field rule
//! runs only on a program with no other error: a body that failed to check
//! may read a field the checker could not resolve.

use std::collections::HashSet;

use oska_syntax::ast::{
    Block, Expr, ExprKind, Fields, FnDecl, ItemKind, Module, Param, Pattern, PatternKind, Stmt,
    StmtKind, StrPart,
};
use oska_syntax::{Applicability, Diagnostic, Edit, Span};

use crate::authority;
use crate::codes;
use crate::collect::Decls;
use crate::items::Lower;

/// Report every unused capability-bearing parameter in `module`.
pub fn module(src: &str, module: &Module, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => function(src, f, Lower::new(decls), diagnostics),
            // An inherent impl's methods are the impl's own; a trait
            // impl's signatures are the trait's.
            ItemKind::Impl(imp) if imp.trait_ref.is_none() => {
                let lower = Lower::new(decls).with_generics(&imp.generics);
                let target = lower.ty(&imp.target);
                let lower = lower.with_self(target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        function(src, f, lower.clone(), diagnostics);
                    }
                }
            }
            _ => {}
        }
    }
}

fn function(src: &str, f: &FnDecl, lower: Lower, diagnostics: &mut Vec<Diagnostic>) {
    let Some(body) = &f.body else {
        return;
    };
    let bearing = authority::bearing_params(f, lower);
    if bearing.is_empty() {
        return;
    }
    let mut read = Uses {
        params: bearing.iter().map(|p| p.name.name.as_str()).collect(),
        ..Uses::default()
    };
    // A contract may read a parameter of a bounded type parameter, which
    // `E0274` keeps pure there; removing it would break the contract.
    for r in &f.requires {
        read.expr(&r.cond);
    }
    for e in &f.ensures {
        read.scoped(|this| {
            this.bound.push(&e.binding.name);
            this.expr(&e.cond);
        });
    }
    read.block(body);
    let is_main = f.name.name == "main";
    for param in bearing {
        if read.used.contains(param.name.name.as_str()) {
            continue;
        }
        let name = &param.name.name;
        let message = format!(
            "unused authority: `{}` never uses its parameter `{name}`, so it declares \
             authority it does not exercise; remove the parameter",
            f.name.name
        );
        let applicability = if is_main {
            // Nothing calls `main`, so nothing else changes.
            Applicability::Machine
        } else {
            // Every call site changes too.
            Applicability::Maybe
        };
        diagnostics.push(
            Diagnostic::new(codes::UNUSED_AUTHORITY, message, param.span)
                .with_fix(
                    format!("remove the parameter `{name}`"),
                    vec![Edit::delete(removal(src, f, param))],
                )
                .applicable(applicability),
        );
    }
}

/// The text to delete to remove `param` from `f`'s parameter list, with
/// the comma that separates it from its neighbour.
fn removal(src: &str, f: &FnDecl, param: &Param) -> Span {
    let index = f
        .params
        .iter()
        .position(|p| std::ptr::eq(p, param))
        .expect("a parameter of the function");
    let spans: Vec<Span> = f.params.iter().map(|p| p.span).collect();
    let receiver = f.receiver.as_ref().map(|r| r.span.end);
    list_removal(src, &spans, index, receiver)
}

/// The text to delete to remove item `index` of a comma-separated list
/// whose items span `spans`, with the comma that separates it from its
/// neighbour. `before` is the end of an item ahead of the list's first
/// one, such as a method's receiver.
fn list_removal(src: &str, spans: &[Span], index: usize, before: Option<usize>) -> Span {
    let span = spans[index];
    if let Some(next) = spans.get(index + 1) {
        return Span {
            start: span.start,
            end: next.start,
        };
    }
    let previous = match index {
        0 => before,
        _ => Some(spans[index - 1].end),
    };
    if let Some(end) = previous {
        return Span {
            start: end,
            end: span.end,
        };
    }
    // The only item: empty the brackets, trailing comma and all.
    let bytes = src.as_bytes();
    let mut start = span.start;
    while start > 0 && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    let mut end = span.end;
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    if bytes.get(end) == Some(&b',') {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
    }
    Span { start, end }
}

/// Report every capability-bearing field of a `cap struct` in `module`
/// that is not in `reads`, the `(struct key, field index)` of every field
/// some code reads.
pub fn fields(
    src: &str,
    module: &Module,
    decls: &Decls,
    reads: &HashSet<(String, usize)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in &module.items {
        let ItemKind::Struct(s) = &item.kind else {
            continue;
        };
        if !s.cap {
            continue;
        }
        let list = match &s.fields {
            Fields::Named(list) | Fields::Positional(list) => list,
            Fields::Unit => continue,
        };
        let lower = Lower::new(decls).with_generics(&s.generics);
        let spans: Vec<Span> = list.iter().map(|f| f.span).collect();
        for (index, field) in list.iter().enumerate() {
            if reads.contains(&(s.name.name.clone(), index))
                || !authority::bearing_field(&field.ty, &lower)
            {
                continue;
            }
            let what = match &field.name {
                Some(name) => format!("field `{}`", name.name),
                None => format!("field {index}"),
            };
            let message = format!(
                "unused authority: no code reads the {what} of `{}`, so it holds authority \
                 the program does not exercise; remove the field",
                s.name.name
            );
            diagnostics.push(
                Diagnostic::new(codes::UNUSED_FIELD_AUTHORITY, message, field.span)
                    .with_fix(
                        format!("remove the {what}"),
                        vec![Edit::delete(list_removal(src, &spans, index, None))],
                    )
                    // Every constructor and pattern of the struct changes too.
                    .applicable(Applicability::Maybe),
            );
        }
    }
}

/// Which of a function's capability-bearing parameters its body uses.
#[derive(Default)]
struct Uses<'a> {
    /// The parameters' names.
    params: HashSet<&'a str>,
    /// The names bound inside the body so far, innermost last: a read of
    /// one of these is not a read of the parameter it shadows.
    bound: Vec<&'a str>,
    /// The parameters read.
    used: HashSet<&'a str>,
}

impl<'a> Uses<'a> {
    fn read(&mut self, name: &'a str) {
        if self.params.contains(name) && !self.bound.contains(&name) {
            self.used.insert(name);
        }
    }

    /// Walk `f` with the names bound inside it dropped again afterwards.
    fn scoped(&mut self, f: impl FnOnce(&mut Self)) {
        let depth = self.bound.len();
        f(self);
        self.bound.truncate(depth);
    }

    fn block(&mut self, block: &'a Block) {
        self.scoped(|this| {
            for stmt in &block.stmts {
                this.stmt(stmt);
            }
        });
    }

    fn stmt(&mut self, stmt: &'a Stmt) {
        match &stmt.kind {
            StmtKind::Let { name, value, .. } => {
                // `let _ = name` only discards the value.
                let discard = name.name == "_" && matches!(value.kind, ExprKind::Name(_));
                if !discard {
                    self.expr(value);
                }
                self.bound.push(&name.name);
            }
            StmtKind::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                self.expr(iter);
                self.scoped(|this| {
                    this.bound.push(&binding.name);
                    this.block(body);
                });
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    fn pattern(&mut self, pattern: &'a Pattern) {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
            PatternKind::Binding(name) => self.bound.push(&name.name),
            PatternKind::Constructor { args, .. } => {
                for arg in args {
                    self.pattern(&arg.pattern);
                }
            }
        }
    }

    fn expr(&mut self, expr: &'a Expr) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::SinkStr { .. }
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::SelfValue => {}
            ExprKind::Name(name) => self.read(&name.name),
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Hole { path, .. } = part {
                        if let Some(first) = path.first() {
                            self.read(&first.name);
                        }
                    }
                }
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(&arg.value);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Try(inner) => self.expr(inner),
            ExprKind::Range { start, end } => {
                self.expr(start);
                self.expr(end);
            }
            ExprKind::List(items) => items.iter().for_each(|e| self.expr(e)),
            ExprKind::Closure { params, body } => self.scoped(|this| {
                this.bound
                    .extend(params.iter().map(|p| p.name.name.as_str()));
                this.expr(body);
            }),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch);
                }
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Scope { handle, body } => self.scoped(|this| {
                this.bound.push(&handle.name);
                this.block(body);
            }),
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.scoped(|this| {
                        this.pattern(&arm.pattern);
                        if let Some(guard) = &arm.guard {
                            this.expr(guard);
                        }
                        this.expr(&arm.body);
                    });
                }
            }
        }
    }
}
