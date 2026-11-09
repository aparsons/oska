//! Secrets: what a `Secret` cannot do.
//!
//! - It cannot be interpolated (`E0350`).
//! - It is not `Eq`: `==` and `!=` on one leak timing information
//!   (`E0351`). The fix rewrites the comparison to `constant_time_eq`.
//! - It is not `Display`, so it cannot be printed (`E0352`).
//!
//! Its value leaves the wrapper only through `.expose()`, an audit site
//! (`body/audit.rs`).

use oska_syntax::ast::{BinaryOp, Expr, ExprKind, Ident};
use oska_syntax::{Diagnostic, Edit, Span};

use super::{literal_last, Body};
use crate::codes;
use crate::ty::{self, Ty};

/// The traits a secret deliberately lacks: each with the code and
/// message reported when one is needed.
const REFUSED: &[(&str, &str, &str)] = &[
    (
        ty::EQ,
        codes::SECRET_COMPARED,
        "a `Secret` is not `Eq`, because comparing one can leak timing information; \
         use `constant_time_eq`",
    ),
    (
        ty::DISPLAY,
        codes::SECRET_DISPLAYED,
        "a `Secret` is not `Display`, so it cannot be printed; read its value with \
         `.expose()` if it must be shown",
    ),
];

impl Body<'_, '_, '_> {
    /// Whether `ty` is a `Secret`.
    fn is_secret(&self, ty: &Ty) -> bool {
        self.infer.resolve(ty).head() == Some(ty::SECRET)
    }

    /// Whether a value of type `ty` holds a `Secret` anywhere inside: in a
    /// type argument, or in a field of a struct or a variant of an enum.
    fn holds_secret(&self, ty: &Ty) -> bool {
        self.holds_secret_in(&self.infer.resolve(ty), &mut Vec::new())
    }

    /// [`Body::holds_secret`], skipping the types in `seen` so that a
    /// recursive type terminates.
    fn holds_secret_in(&self, ty: &Ty, seen: &mut Vec<Ty>) -> bool {
        let Ty::Named(name, args, _) = ty else {
            return false;
        };
        if name == ty::SECRET {
            return true;
        }
        if seen.contains(ty) {
            return false;
        }
        seen.push(ty.clone());
        if args.iter().any(|a| self.holds_secret_in(a, seen)) {
            return true;
        }
        let (generics, shapes) = match (self.items.structs.get(name), self.items.enums.get(name)) {
            (Some(info), _) => (&info.generics, vec![&info.fields]),
            (None, Some(info)) => (
                &info.generics,
                info.variants.iter().map(|(_, f)| f).collect(),
            ),
            (None, None) => return false,
        };
        let subst: Vec<(String, Ty)> = generics.iter().cloned().zip(args.iter().cloned()).collect();
        shapes
            .into_iter()
            .flat_map(|fields| fields.instantiate(&subst))
            .any(|(_, field)| self.holds_secret_in(&field, seen))
    }

    /// Check the interpolation hole at `span`, of type `ty`: it must not
    /// hold a secret anywhere (`E0350`), and it must be `Display`.
    pub(super) fn interpolated_hole(&mut self, ty: &Ty, span: Span) {
        if self.holds_secret(ty) {
            self.error(
                codes::SECRET_INTERPOLATED,
                "a `Secret` cannot be interpolated, nor a value that holds one; read its \
                 value with `.expose()` if it must appear in text",
                span,
            );
            return;
        }
        self.require_trait(ty, ty::DISPLAY, &[], span, "interpolation");
    }

    /// Report the bound `trait_name` that a secret of type `ty` fails at
    /// `span`, with the secret's own code. Returns whether it reported.
    pub(super) fn secret_bound(&mut self, ty: &Ty, trait_name: &str, span: Span) -> bool {
        if !self.is_secret(ty) {
            return false;
        }
        let Some(&(_, code, message)) = REFUSED.iter().find(|(t, _, _)| *t == trait_name) else {
            return false;
        };
        self.error(code, message, span);
        true
    }

    /// Report the method `name` that a secret of type `receiver` lacks
    /// because it is a method of a trait in [`REFUSED`]. Returns whether
    /// it reported.
    pub(super) fn secret_method(&mut self, receiver: &Ty, name: &Ident) -> bool {
        let refused = REFUSED.iter().find(|(trait_name, _, _)| {
            self.items
                .traits
                .get(*trait_name)
                .is_some_and(|info| info.methods.iter().any(|(sig, _)| sig.name == name.name))
        });
        match refused {
            Some(&(_, code, message)) if self.is_secret(receiver) => {
                self.error(code, message, name.span);
                true
            }
            _ => false,
        }
    }

    /// The operands of `expr`, `lhs op rhs` where `op` is `==` or `!=`,
    /// which must have the same type: that type, or `None` when they
    /// don't have one or one is a secret (`E0351`, with a fix to
    /// `constant_time_eq`). A literal operand takes the other operand's
    /// type, so `x == 1` works for any integer `x`.
    pub(super) fn compared_operands(
        &mut self,
        expr: &Expr,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Option<Ty> {
        let (first, second) = literal_last(lhs, rhs);
        let first_ty = self.infer(first);
        let first_ty = self.infer.resolve(&first_ty);
        if first_ty.is_error() {
            self.infer(second);
            return None;
        }
        let first_secret = self.is_secret(&first_ty);
        let expected = (!first_secret).then_some(&first_ty);
        let second_ty = self.expr(second, expected);
        let second_secret = self.is_secret(&second_ty);
        if !first_secret && !second_secret {
            // Comparing inspects text without passing it on, so the
            // operands may differ in trust.
            let loosened = self.infer.loosen(&first_ty);
            let ty = self.expect(&second_ty, &loosened, second.span);
            return (!ty.is_error()).then_some(first_ty);
        }
        let (secret, other, other_secret) = if first_secret {
            (first, second, second_secret)
        } else {
            (second, first, false)
        };
        let fix = self.constant_time_eq(op, secret, other, other_secret);
        self.diagnostics.push(
            Diagnostic::new(
                codes::SECRET_COMPARED,
                format!(
                    "a `Secret` cannot be compared with `{}`, because that can leak timing \
                     information; use `constant_time_eq`",
                    op.text()
                ),
                expr.span,
            )
            .with_fix(
                "compare in constant time",
                vec![Edit {
                    span: expr.span,
                    replacement: fix,
                }],
            ),
        );
        None
    }

    /// `secret.constant_time_eq(other)`, negated for `!=`. Two secrets
    /// compare with `constant_time_eq_secret`, so neither is exposed.
    fn constant_time_eq(
        &self,
        op: BinaryOp,
        secret: &Expr,
        other: &Expr,
        other_secret: bool,
    ) -> String {
        let not = if op == BinaryOp::Ne { "!" } else { "" };
        let receiver = self.postfix_operand(secret);
        let method = if other_secret {
            "constant_time_eq_secret"
        } else {
            "constant_time_eq"
        };
        let argument = &self.src[other.span.start..other.span.end];
        format!("{not}{receiver}.{method}({argument})")
    }

    /// The source of `expr`, parenthesized unless a method call can
    /// follow it as written.
    fn postfix_operand(&self, expr: &Expr) -> String {
        let text = &self.src[expr.span.start..expr.span.end];
        match expr.kind {
            ExprKind::Name(_)
            | ExprKind::SelfValue
            | ExprKind::Field { .. }
            | ExprKind::Call { .. }
            | ExprKind::Try(_) => text.to_string(),
            _ => format!("({text})"),
        }
    }
}
