//! The `Result` discipline: `?` on a `Result` or an `Option`, error
//! conversion through `From`, and dropped `Result` values.

use oska_syntax::ast::Expr;
use oska_syntax::Span;

use super::Body;
use crate::codes;
use crate::infer::Mismatch;
use crate::ty::{self, Ty};

impl Body<'_, '_, '_> {
    /// `operand?` (the whole expression is `expr`): the success value of a
    /// `Result` or an `Option`, whose failure the enclosing function (or
    /// closure) returns. That function must return the same kind: `?`
    /// never crosses between `Option` and `Result` (`E0243`). A `Result`'s
    /// error type goes through [`Body::propagate_error`].
    pub(super) fn try_expr(&mut self, expr: &Expr, operand: &Expr) -> Ty {
        let ty = self.infer(operand);
        let (kind, args) = match self.infer.resolve(&ty) {
            Ty::Named(name, args)
                if (name == ty::RESULT && args.len() == 2)
                    || (name == ty::OPTION && args.len() == 1) =>
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
                let failure = if kind == ty::RESULT {
                    Ty::Named(kind.clone(), vec![self.infer.fresh(), args[1].clone()])
                } else {
                    Ty::Named(kind.clone(), vec![self.infer.fresh()])
                };
                self.infer.unify(&failure, &ret);
            }
            Ty::Named(name, ret_args) if name == kind => {
                if kind == ty::RESULT && ret_args.len() == 2 {
                    self.propagate_error(&args[1], &ret_args[1], expr.span);
                }
            }
            other => {
                let advice = match (kind.as_str(), other.head()) {
                    (ty::OPTION, Some(ty::RESULT)) => {
                        "; convert it explicitly, for example with `.ok_or(err)?`"
                    }
                    (ty::RESULT, Some(ty::OPTION)) => {
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
    pub(super) fn propagate_error(&mut self, from: &Ty, to: &Ty, span: Span) {
        match self.infer.flow(from, to) {
            Ok(()) => return,
            Err(Mismatch::CapLost) => {
                self.mismatch(Mismatch::CapLost, from, to, span);
                return;
            }
            Err(Mismatch::Types) => {}
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

    /// Whether `?` can convert an error of type `from` into `to`: `to`
    /// implements `From<from>`, through an explicit `impl` or a bound.
    pub(super) fn converts_from(&self, from: &Ty, to: &Ty) -> bool {
        self.implements(to, ty::FROM, std::slice::from_ref(from))
    }

    /// An expression whose value is discarded. Dropping a `Result` is an
    /// error; `let _ = f()` discards one deliberately.
    pub(super) fn expr_stmt(&mut self, expr: &Expr) {
        let ty = self.infer(expr);
        if let Ty::Named(name, _) = self.infer.resolve(&ty) {
            if name == ty::RESULT {
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
