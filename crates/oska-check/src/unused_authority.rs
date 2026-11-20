//! **Unused authority** (ADR 0016): under `--release`, a
//! capability-bearing parameter that its function never uses is an error.
//!
//! Any read of the parameter's name in the function counts as a use: a
//! method call on it, passing it on, attenuating it, storing it,
//! returning it, capturing it in a closure, or naming it in a contract.
//! The rule is deliberately coarse, so it has no false positives: it does
//! not try to prove the capability is reached, and a name that a `let` or
//! a closure parameter shadows still counts as read.
//!
//! What is capability-bearing is [`authority::bearing_params`], the
//! authority report's own definition. A method whose signature a trait
//! fixes is exempt: an `impl Trait for Type` method, and a trait's default
//! method. There is no `_name` escape hatch. The receiver is not a
//! parameter that can be removed, so it is not judged.

use std::collections::HashSet;

use oska_syntax::ast::{Block, Expr, ExprKind, FnDecl, ItemKind, Module, Param, StmtKind, StrPart};
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
    let mut read = Names::default();
    for r in &f.requires {
        read.expr(&r.cond);
    }
    for e in &f.ensures {
        read.expr(&e.cond);
    }
    read.block(body);
    let is_main = f.name.name == "main";
    for param in bearing {
        if read.0.contains(param.name.name.as_str()) {
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
    if let Some(next) = f.params.get(index + 1) {
        return Span {
            start: param.span.start,
            end: next.span.start,
        };
    }
    let previous = match index {
        0 => f.receiver.as_ref().map(|r| r.span.end),
        _ => Some(f.params[index - 1].span.end),
    };
    if let Some(end) = previous {
        return Span {
            start: end,
            end: param.span.end,
        };
    }
    // The only parameter: empty the parentheses, trailing comma and all.
    let bytes = src.as_bytes();
    let mut start = param.span.start;
    while start > 0 && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    let mut end = param.span.end;
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

/// Every name a function reads.
#[derive(Default)]
struct Names<'a>(HashSet<&'a str>);

impl<'a> Names<'a> {
    fn block(&mut self, block: &'a Block) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StmtKind::Let { value, .. } => self.expr(value),
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
                StmtKind::For { iter, body, .. } => {
                    self.expr(iter);
                    self.block(body);
                }
                StmtKind::While { cond, body } => {
                    self.expr(cond);
                    self.block(body);
                }
                StmtKind::Expr(e) => self.expr(e),
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
            ExprKind::Name(name) => {
                self.0.insert(&name.name);
            }
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Hole { path, .. } = part {
                        if let Some(first) = path.first() {
                            self.0.insert(&first.name);
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
            ExprKind::Closure { body, .. } => self.expr(body),
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
            ExprKind::Scope { body, .. } => self.block(body),
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expr(guard);
                    }
                    self.expr(&arm.body);
                }
            }
        }
    }
}
