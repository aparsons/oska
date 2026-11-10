//! The `Result` discipline: `?` on a `Result` or an `Option`, error
//! conversion through `From`, and dropped `Result` values.

use oska_syntax::ast::Expr;
use oska_syntax::Span;

use super::Body;
use crate::codes;
use crate::infer::Mismatch;
use crate::program::lower::Resolution;
use crate::program::Callee;
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
            Ty::Named(name, args, _)
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
                    Ty::generic(&kind, vec![self.infer.fresh(), args[1].clone()])
                } else {
                    Ty::generic(&kind, vec![self.infer.fresh()])
                };
                self.infer.unify(&failure, &ret);
                self.record(expr.span, Resolution::Try(None));
            }
            Ty::Named(name, ret_args, _) if name == kind => {
                if kind == ty::RESULT && ret_args.len() == 2 {
                    if let Some(convert) = self.propagate_error(&args[1], &ret_args[1], expr.span) {
                        self.record(expr.span, Resolution::Try(convert));
                    }
                } else {
                    self.record(expr.span, Resolution::Try(None));
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
    ///
    /// Returns how the checked program converts the error: `Some(None)`
    /// when it passes as it is, `Some(Some(from))` through the `From`
    /// impl that `to` has, and `None` when it cannot be propagated or
    /// converts only through a bound, which the checked program cannot
    /// express yet.
    pub(super) fn propagate_error(
        &mut self,
        from: &Ty,
        to: &Ty,
        span: Span,
    ) -> Option<Option<Callee>> {
        match self.infer.flow(from, to) {
            Ok(()) => return Some(None),
            Err(lost @ (Mismatch::CapLost | Mismatch::ScopeLost | Mismatch::TrustLost)) => {
                self.mismatch(lost, from, to, span);
                return None;
            }
            Err(Mismatch::Types) => {}
        }
        let from = self.infer.resolve(from);
        let to = self.infer.resolve(to);
        if from.is_error() || to.is_error() {
            return None;
        }
        if self.converts_from(&from, &to) {
            let convert = self
                .impl_of(&to, ty::FROM, std::slice::from_ref(&from))
                .and_then(|imp| imp.method_name("from"));
            if convert.is_none() {
                // Only a bound says `to` converts: no impl to call.
                let what = "a `From`-bound conversion".to_string();
                self.record(span, Resolution::Unsupported(what));
            }
            return convert.map(|name| Some(Callee::Function(name)));
        }
        self.error(
            codes::TRY_ERROR_TYPE,
            format!(
                "`?` cannot propagate an error of type `{from}` from a function whose \
                 error type is `{to}`; write `impl From<{from}> for {to}` or convert it explicitly"
            ),
            span,
        );
        None
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
        if let Ty::Named(name, _, _) = self.infer.resolve(&ty) {
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
