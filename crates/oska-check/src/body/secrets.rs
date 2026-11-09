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

use super::Body;
use crate::codes;
use crate::ty::{self, Ty};

/// A trait a secret deliberately lacks, and what is reported when one
/// is needed.
struct Refused {
    trait_name: &'static str,
    code: &'static str,
    message: &'static str,
}

/// The message of `E0351`, wherever a secret is compared.
const COMPARED: &str = "a `Secret` is not `Eq`, because comparing one can leak timing \
                        information; use `constant_time_eq`";

/// The traits a secret deliberately lacks.
const REFUSED: &[Refused] = &[
    Refused {
        trait_name: ty::EQ,
        code: codes::SECRET_COMPARED,
        message: COMPARED,
    },
    Refused {
        trait_name: ty::DISPLAY,
        code: codes::SECRET_DISPLAYED,
        message: "a `Secret` is not `Display`, so it cannot be printed; read its value with \
                  `.expose()` if it must be shown",
    },
];

impl Body<'_, '_, '_> {
    /// Whether `ty` is a `Secret`.
    pub(super) fn is_secret(&self, ty: &Ty) -> bool {
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
        let Some(refused) = REFUSED.iter().find(|r| r.trait_name == trait_name) else {
            return false;
        };
        self.error(refused.code, refused.message, span);
        true
    }

    /// Report the method `name` that a secret of type `receiver` lacks
    /// because it is a method of a trait in [`REFUSED`]. Returns whether
    /// it reported.
    pub(super) fn secret_method(&mut self, receiver: &Ty, name: &Ident) -> bool {
        let refused = REFUSED.iter().find(|r| {
            self.items
                .traits
                .get(r.trait_name)
                .is_some_and(|info| info.methods.iter().any(|(sig, _)| sig.name == name.name))
        });
        match refused {
            Some(refused) if self.is_secret(receiver) => {
                self.error(refused.code, refused.message, name.span);
                true
            }
            _ => false,
        }
    }

    /// Report `expr`, `lhs op rhs` where `op` is `==` or `!=` and the
    /// operand `secret` is a secret (`E0351`), with a fix that compares
    /// in constant time. `other_secret` says whether `other` is one too.
    pub(super) fn secret_compared(
        &mut self,
        expr: &Expr,
        op: BinaryOp,
        secret: &Expr,
        other: &Expr,
        other_secret: bool,
    ) {
        let fix = self.constant_time_eq(op, secret, other, other_secret);
        self.diagnostics.push(
            Diagnostic::new(codes::SECRET_COMPARED, COMPARED, expr.span).with_fix(
                "compare in constant time",
                vec![Edit {
                    span: expr.span,
                    replacement: fix,
                }],
            ),
        );
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
