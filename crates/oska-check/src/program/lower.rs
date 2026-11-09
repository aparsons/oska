//! Lowering a checked module into the **checked program**.
//!
//! Checking bodies records what each name and call resolved to in
//! [`Resolutions`], keyed by the span of the node it was resolved at.
//! Lowering walks the AST and reads those records, so it never resolves
//! anything itself. A node it cannot lower yet becomes
//! [`ExprKind::Unsupported`].

use std::collections::HashMap;

use oska_syntax::ast::{self, ItemKind, Module, StmtKind, StrPart};
use oska_syntax::Span;

use super::{Callee, Expr, ExprKind, Function, Param, Program, Stmt};
use crate::items::Items;

/// What a node resolved to while its body was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A name that is a local: its index in the body's locals stack,
    /// which is its slot. Recorded at the name.
    Local(usize),
    /// A method call reaching this inherent method. Recorded at the
    /// call.
    Method(Callee),
}

/// Every resolution in a module, keyed by the span it was recorded at.
pub type Resolutions = HashMap<Span, Resolution>;

/// Lower every free function of `module`. (Methods are not lowered
/// yet.)
pub fn module(module: &Module, items: &Items, resolutions: &Resolutions) -> Program {
    let lowering = Lowering { resolutions };
    let mut program = Program::default();
    for item in &module.items {
        let ItemKind::Fn(f) = &item.kind else {
            continue;
        };
        let Some(body) = &f.body else {
            continue;
        };
        let name = f.name.name.clone();
        let types = items.fns.get(&name).map(|sig| &sig.params);
        let params = f
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| Param {
                name: p.name.name.clone(),
                ty: types
                    .and_then(|t| t.get(i))
                    .map_or_else(String::new, |(_, ty)| ty.to_string()),
                binds: p.name.name != "_",
            })
            .collect();
        let function = Function {
            name: name.clone(),
            params,
            body: lowering.block(body),
        };
        program.functions.insert(name, function);
    }
    program
}

struct Lowering<'a> {
    resolutions: &'a Resolutions,
}

impl Lowering<'_> {
    fn block(&self, block: &ast::Block) -> Expr {
        let mut stmts: Vec<Stmt> = Vec::new();
        let mut value = None;
        for (i, stmt) in block.stmts.iter().enumerate() {
            let last = i + 1 == block.stmts.len();
            match &stmt.kind {
                StmtKind::Expr(e) if last => value = Some(Box::new(self.expr(e))),
                StmtKind::Expr(e) => stmts.push(Stmt::Expr(self.expr(e))),
                _ => stmts.push(Stmt::Expr(Expr {
                    kind: ExprKind::Unsupported,
                    span: stmt.span,
                })),
            }
        }
        Expr {
            kind: ExprKind::Block { stmts, value },
            span: block.span,
        }
    }

    fn expr(&self, expr: &ast::Expr) -> Expr {
        let kind = match &expr.kind {
            ast::ExprKind::Unit => ExprKind::Unit,
            ast::ExprKind::Str(parts) => match parts.as_slice() {
                [] => ExprKind::Str(String::new()),
                [StrPart::Text { value, .. }] => ExprKind::Str(value.clone()),
                _ => ExprKind::Unsupported,
            },
            ast::ExprKind::Name(ident) => match self.resolutions.get(&ident.span) {
                Some(Resolution::Local(slot)) => ExprKind::Local(*slot),
                _ => ExprKind::Unsupported,
            },
            ast::ExprKind::Block(block) => return self.block(block),
            ast::ExprKind::Call { callee, args } => {
                match (&callee.kind, self.resolutions.get(&expr.span)) {
                    (ast::ExprKind::Field { base, .. }, Some(Resolution::Method(callee))) => {
                        let mut lowered = vec![self.expr(base)];
                        lowered.extend(args.iter().map(|a| self.expr(&a.value)));
                        ExprKind::Call {
                            callee: callee.clone(),
                            args: lowered,
                        }
                    }
                    _ => ExprKind::Unsupported,
                }
            }
            _ => ExprKind::Unsupported,
        };
        Expr {
            kind,
            span: expr.span,
        }
    }
}
