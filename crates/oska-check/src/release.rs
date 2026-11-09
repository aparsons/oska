//! Release-only rules: `dbg(...)` and `todo()` are errors under
//! `--release`.
//!
//! This is a syntactic walk over every body, separate from body
//! checking: any call whose callee is the bare name `dbg` or `todo` is
//! reported.

use oska_syntax::ast::{Block, Expr, ExprKind, FnDecl, Item, ItemKind, Module, StmtKind};
use oska_syntax::Diagnostic;

use crate::codes;

/// Report every `dbg(...)` and `todo()` call in `module`.
pub fn module(module: &Module, diagnostics: &mut Vec<Diagnostic>) {
    let mut walk = Walk { diagnostics };
    for item in &module.items {
        walk.item(item);
    }
}

struct Walk<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl Walk<'_> {
    fn item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(f) => self.function(f),
            ItemKind::Const(c) => self.expr(&c.value),
            ItemKind::Test(t) => self.block(&t.body),
            ItemKind::Impl(imp) => imp.methods.iter().for_each(|m| self.item(m)),
            ItemKind::Trait(t) => t.methods.iter().for_each(|m| self.item(m)),
            ItemKind::Use(_) | ItemKind::Struct(_) | ItemKind::Enum(_) => {}
        }
    }

    fn function(&mut self, f: &FnDecl) {
        for r in &f.requires {
            self.expr(&r.cond);
        }
        for e in &f.ensures {
            self.expr(&e.cond);
        }
        if let Some(body) = &f.body {
            self.block(body);
        }
    }

    fn block(&mut self, block: &Block) {
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

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Str(_)
            | ExprKind::SinkStr { .. }
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Name(_)
            | ExprKind::SelfValue => {}
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Call { callee, args } => {
                match &callee.kind {
                    ExprKind::Name(name) if name.name == "dbg" => {
                        self.diagnostics.push(Diagnostic::new(
                            codes::DBG_IN_RELEASE,
                            "`dbg(...)` is not allowed in a release build",
                            expr.span,
                        ));
                    }
                    ExprKind::Name(name) if name.name == "todo" => {
                        self.diagnostics.push(Diagnostic::new(
                            codes::TODO_IN_RELEASE,
                            "`todo()` is not allowed in a release build; finish this code first",
                            expr.span,
                        ));
                    }
                    _ => {}
                }
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
