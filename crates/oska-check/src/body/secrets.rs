// SPDX-License-Identifier: Apache-2.0
//! Secrets: what a `Secret` cannot do.
//!
//! - It cannot be interpolated (`E0350`).
//! - It is not `Eq`: `==` and `!=` on one leak timing information
//!   (`E0351`). The fix rewrites the comparison to `constant_time_eq`, or
//!   for a secret integer to `ct_eq`.
//! - It is not `Display`, so it cannot be printed (`E0352`).
//! - It cannot decide a branch (`E0353`): a condition, a `match`, a range
//!   bound, or an operand of `&&`, `||` or `!`.
//! - It cannot decide a memory address (`E0354`): an index, an offset or
//!   a length, where a parameter is an `Int`, or a hash key, since it is
//!   not `Hash`.
//! - It cannot decide a variable-time instruction or a panic (`E0355`):
//!   an operand of `/` or `%`, or a shift or rotation amount.
//! - It cannot be ordered (`E0356`): `<`, `<=`, `>` and `>=`.
//! - It cannot hold a `Float` (`E0357`): `Secret<Float>` is refused, as a
//!   written type and as `Secret.new` of a float, because float arithmetic
//!   is not constant-time on common hardware (ADR 0027).
//!
//! A secret integer (`Secret<U8>`, `Secret<U32>`, `Secret<U64>`) has the
//! wrapping arithmetic and bitwise operators, and a public operand of its
//! type joins it to a secret result ([`Body::secret_operands`]). Its value
//! leaves the wrapper only through `.expose()`, an audit site
//! (`body/audit.rs`).

use oska_syntax::ast::{BinaryOp, Expr, ExprKind, Ident};
use oska_syntax::{Applicability, Diagnostic, Edit, Span};

use super::{Body, Deferred};
use crate::codes;
use crate::resolve::SECRET_FLOAT;
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
                        information; use `constant_time_eq`, or `ct_eq` on a secret integer";

/// The traits a secret deliberately lacks.
const REFUSED: &[Refused] = &[
    Refused {
        trait_name: ty::EQ,
        code: codes::SECRET_COMPARED,
        message: COMPARED,
    },
    Refused {
        trait_name: ty::HASH,
        code: codes::SECRET_ADDRESS,
        message: "a `Secret` is not `Hash`: as a hash key it would decide where in memory a \
                  value is kept",
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
        let (generics, shapes) = match (
            self.items.structs.get(name.as_str()),
            self.items.enums.get(name.as_str()),
        ) {
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
        self.require_trait(ty, ty::DISPLAY, &[], span, "interpolation", false);
    }

    /// Report the bound `trait_name` that a secret of type `ty` fails at
    /// `span`, with the secret's own code. Returns whether it reported.
    pub(super) fn secret_bound(&mut self, ty: &Ty, trait_name: &str, span: Span) -> bool {
        // A type that holds a secret is not `Hash` either: its derived hash
        // would be as much of an address as the secret's own.
        let holds = if trait_name == ty::HASH {
            self.holds_secret(ty)
        } else {
            self.is_secret(ty)
        };
        if !holds {
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
        secret: (&Expr, &Ty),
        other: &Expr,
        other_secret: bool,
    ) {
        let (fix, applicability) = self.constant_time_eq(op, secret, other, other_secret);
        self.out.diagnostics.push(
            Diagnostic::new(codes::SECRET_COMPARED, COMPARED, expr.span)
                .with_fix(
                    "compare in constant time",
                    vec![Edit {
                        span: expr.span,
                        replacement: fix,
                    }],
                )
                .applicable(applicability),
        );
    }

    /// `secret.constant_time_eq(other)`, negated for `!=`, and how safe it
    /// is to apply. Two secrets compare with `constant_time_eq_secret`, so
    /// neither is exposed. A secret integer compares with `ct_eq`, and a
    /// `Secret<Bool>` with `xor`, a public operand made secret with
    /// `Secret.new`: those give a `Secret<Bool>` where `==` gave a `Bool`,
    /// so the fixed program may not check, and the fix is `maybe`.
    fn constant_time_eq(
        &self,
        op: BinaryOp,
        (secret, secret_ty): (&Expr, &Ty),
        other: &Expr,
        other_secret: bool,
    ) -> (String, Applicability) {
        let receiver = self.postfix_operand(secret);
        let argument = &self.src[other.span.start..other.span.end];
        let inner = self.secret_inner(secret_ty).map(|t| self.infer.resolve(&t));
        if let Some(inner) = inner.filter(|t| t.has_bitwise_operators() || *t == Ty::bool()) {
            let argument = if other_secret {
                argument.to_string()
            } else {
                format!("Secret.new({argument})")
            };
            let (method, not) = if inner == Ty::bool() {
                ("xor", op == BinaryOp::Eq)
            } else {
                ("ct_eq", op == BinaryOp::Ne)
            };
            let not = if not { ".not()" } else { "" };
            let fix = format!("{receiver}.{method}({argument}){not}");
            return (fix, Applicability::Maybe);
        }
        let not = if op == BinaryOp::Ne { "!" } else { "" };
        let method = if other_secret {
            "constant_time_eq_secret"
        } else {
            "constant_time_eq"
        };
        let fix = format!("{not}{receiver}.{method}({argument})");
        (fix, Applicability::Machine)
    }

    /// The source of `expr`, parenthesized unless a method call can
    /// follow it as written.
    pub(super) fn postfix_operand(&self, expr: &Expr) -> String {
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

/// The operands of an arithmetic, bitwise, shift or ordering operator, as
/// [`Body::secret_operands`] finds them.
pub(super) enum Operands {
    /// Neither is a secret: the first operand's type, which the second has
    /// not been checked against yet.
    Public { first: Ty, second: Ty },
    /// One or both is a secret integer: the result's type.
    Secret(Ty),
    /// Reported already.
    Error,
}

/// The types code makes secrets of with `Secret.new`, and that
/// `ct_select` picks between: the ones a backend keeps in a word, and
/// `Bytes`. (A `Secret<String>` comes only from `Env.secret`.)
const SECRET_TYPES: &[&str] = &["U8", "U32", "U64", "Bool", "Bytes"];

/// Where an argument goes, for what a secret there would decide.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Position {
    /// An index, an offset, a length or a count of a prelude method:
    /// which memory is read, or whether the call panics.
    Address,
    /// The amount of a prelude rotation (`rotl`, `rotr`).
    Rotation,
    /// Anywhere else, as any parameter of a module's own function.
    Plain,
}

impl Position {
    /// The position of the parameter `param` of the callee, which is the
    /// prelude or `std` function `intrinsic` when it is one.
    pub(super) fn of(intrinsic: Option<&str>, param: &Option<String>) -> Position {
        match (intrinsic, param.as_deref()) {
            (Some("rotl" | "rotr"), Some("n")) => Position::Rotation,
            (Some(_), Some("index" | "offset" | "start" | "end" | "len" | "times")) => {
                Position::Address
            }
            _ => Position::Plain,
        }
    }
}

impl Body<'_, '_, '_> {
    /// The `T` of a `Secret<T>`, or `None` for any other type.
    pub(super) fn secret_inner(&self, ty: &Ty) -> Option<Ty> {
        match self.infer.resolve(ty) {
            Ty::Named(name, args, _) if name == ty::SECRET && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        }
    }

    /// Check `cond`, which decides a branch as `what` (such as "an `if`
    /// condition"): it must be `Bool`, and a secret is `E0353`.
    pub(super) fn condition(&mut self, cond: &Expr, what: &str) -> Ty {
        // No expectation guides it: `Bool` would instantiate a generic
        // call's result as `Bool` where it is a `Secret<Bool>`.
        let ty = self.infer(cond);
        if self.is_secret(&ty) {
            self.secret_branch(what, cond.span, None);
            return Ty::Error;
        }
        self.expect(&ty, &Ty::bool(), cond.span)
    }

    /// Report a secret that would decide a branch as `what` (`E0353`), at
    /// `span`, with `fix` as the replacement of the spanned source.
    pub(super) fn secret_branch(&mut self, what: &str, span: Span, fix: Option<String>) {
        let mut diagnostic = Diagnostic::new(
            codes::SECRET_BRANCH,
            format!(
                "a secret cannot be {what}: the branch taken would reveal it; combine secret \
                 conditions with `and`, `or`, `not` and `ct_select`, or read the value with \
                 `.expose()`"
            ),
            span,
        );
        if let Some(replacement) = fix {
            diagnostic = diagnostic.with_fix(
                "compute on the secret without a branch",
                vec![Edit { span, replacement }],
            );
        }
        self.out.diagnostics.push(diagnostic);
    }

    /// `&&`, `||` or `implies`, `expr`: each operand must be `Bool`, and a
    /// secret one is `E0353`. When both are secret, the fix rewrites `&&`
    /// to `and` and `||` to `or`, which evaluate both.
    pub(super) fn logical(&mut self, expr: &Expr, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Ty {
        let lhs_ty = self.infer(lhs);
        let rhs_ty = self.infer(rhs);
        let (lhs_secret, rhs_secret) = (self.is_secret(&lhs_ty), self.is_secret(&rhs_ty));
        if !lhs_secret && !rhs_secret {
            self.expect(&lhs_ty, &Ty::bool(), lhs.span);
            self.expect(&rhs_ty, &Ty::bool(), rhs.span);
            return Ty::bool();
        }
        let method = match op {
            BinaryOp::And => Some("and"),
            BinaryOp::Or => Some("or"),
            _ => None,
        };
        let fix = method.filter(|_| lhs_secret && rhs_secret).map(|method| {
            let receiver = self.postfix_operand(lhs);
            let argument = &self.src[rhs.span.start..rhs.span.end];
            format!("{receiver}.{method}({argument})")
        });
        self.secret_branch(&format!("an operand of `{}`", op.text()), expr.span, fix);
        Ty::Error
    }

    /// `!operand`, `expr`: a secret operand is `E0353`, with a fix to
    /// `not`.
    pub(super) fn negation(&mut self, expr: &Expr, operand: &Expr) -> Ty {
        let ty = self.infer(operand);
        if self.is_secret(&ty) {
            let fix = format!("{}.not()", self.postfix_operand(operand));
            self.secret_branch("the operand of `!`", expr.span, Some(fix));
            return Ty::Error;
        }
        self.expect(&ty, &Ty::bool(), operand.span)
    }

    /// The operands of `expr`, `lhs op rhs`, where `op` is arithmetic,
    /// bitwise, a shift or an ordering. A literal goes second, so it
    /// takes the other operand's type, the inner type of a secret one.
    ///
    /// When either is a secret: an ordering is `E0356`, a division or a
    /// remainder, or a secret shift amount, `E0355`; otherwise both must
    /// have one secret integer type, or one that type and the other its
    /// public type, and the result is that secret type.
    pub(super) fn secret_operands(
        &mut self,
        expr: &Expr,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Ty>,
    ) -> Operands {
        let expected = expected
            .map(|e| {
                self.secret_inner(e)
                    .unwrap_or_else(|| self.infer.shallow(e))
            })
            .filter(|e| self.infer.shallow(e).is_integer());
        let (first, second) = super::literal_last(lhs, rhs);
        let first_ty = self.expr(first, expected.as_ref());
        let first_inner = self.secret_inner(&first_ty);
        let public = first_inner.clone().unwrap_or_else(|| first_ty.clone());
        let second_ty = self.expr(second, Some(&public));
        let second_inner = self.secret_inner(&second_ty);
        if first_inner.is_none() && second_inner.is_none() {
            return Operands::Public {
                first: first_ty,
                second: second_ty,
            };
        }
        let text = op.text();
        let rhs_secret = if std::ptr::eq(second, rhs) {
            second_inner.is_some()
        } else {
            first_inner.is_some()
        };
        match op {
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                self.error(
                    codes::SECRET_ORDERED,
                    format!(
                        "`{text}` on a secret: ordering secrets would reveal them through a \
                         branch; compare them for equality with `ct_eq`, or read the values \
                         with `.expose()`"
                    ),
                    expr.span,
                );
                return Operands::Error;
            }
            BinaryOp::Div | BinaryOp::Rem => {
                self.error(
                    codes::SECRET_VARIABLE_TIME,
                    format!(
                        "`{text}` on a secret: division takes time that depends on its \
                         operands, and panics on a zero divisor, so either would reveal the \
                         secret; read the value with `.expose()` if it may be public"
                    ),
                    expr.span,
                );
                return Operands::Error;
            }
            BinaryOp::Shl | BinaryOp::Shr if rhs_secret => {
                self.secret_amount(expr.span);
                return Operands::Error;
            }
            _ => {}
        }
        let inner = match (&first_inner, &second_inner) {
            (Some(inner), Some(other)) => {
                self.expect(other, inner, second.span);
                inner.clone()
            }
            (Some(inner), None) => {
                self.expect(&second_ty, inner, second.span);
                inner.clone()
            }
            (None, Some(inner)) => {
                self.expect(&first_ty, inner, first.span);
                inner.clone()
            }
            (None, None) => unreachable!("one operand is a secret"),
        };
        let resolved = self.infer.resolve(&inner);
        if !(resolved.is_error() || resolved.has_bitwise_operators()) {
            let secret = Ty::generic(ty::SECRET, vec![resolved]);
            self.error(
                codes::BAD_OPERAND,
                format!(
                    "`{text}` is not defined for `{secret}`; the operators on secrets are \
                     defined for the secret integers `Secret<U8>`, `Secret<U32>` and \
                     `Secret<U64>` only"
                ),
                expr.span,
            );
            return Operands::Error;
        }
        self.defer(expr.span, &inner, Deferred::Int);
        Operands::Secret(Ty::generic(ty::SECRET, vec![inner]))
    }

    /// `~operand`, `expr`, on a secret integer, whose type `ty` has been
    /// inferred: the result is the same secret type.
    pub(super) fn secret_bit_not(&mut self, expr: &Expr, ty: &Ty) -> Option<Ty> {
        let inner = self.secret_inner(ty)?;
        let resolved = self.infer.resolve(&inner);
        if !(resolved.is_error() || resolved.has_bitwise_operators()) {
            self.error(
                codes::BAD_OPERAND,
                format!(
                    "`~` is not defined for `{}`; the operators on secrets are defined for \
                     the secret integers `Secret<U8>`, `Secret<U32>` and `Secret<U64>` only",
                    self.infer.resolve(ty)
                ),
                expr.span,
            );
            return Some(Ty::Error);
        }
        self.defer(expr.span, &inner, Deferred::Int);
        Some(ty.clone())
    }

    /// Report a secret shift or rotation amount at `span` (`E0355`).
    pub(super) fn secret_amount(&mut self, span: Span) {
        self.error(
            codes::SECRET_VARIABLE_TIME,
            "a secret cannot be a shift or rotation amount: an amount out of range panics, so \
             the panic would reveal it; shift by a public amount",
            span,
        );
    }

    /// Check the argument `value` of a call against its parameter type
    /// `param`, at `position`. A secret where a prelude method wants a
    /// public index, offset or length is `E0354`, and one as the amount
    /// of a rotation is `E0355`. Anywhere else a secret is just not the
    /// `Int` wanted.
    pub(super) fn argument(&mut self, value: &Expr, param: &Ty, position: Position) -> Ty {
        let ty = self.expr(value, Some(param));
        let public = position == Position::Plain
            || !self.is_secret(&ty)
            || self.infer.resolve(param) != Ty::int();
        if public {
            return self.expect(&ty, param, value.span);
        }
        if position == Position::Rotation {
            self.secret_amount(value.span);
        } else {
            self.error(
                codes::SECRET_ADDRESS,
                "a secret cannot be an index, an offset or a length: it would decide which \
                 memory is read, or whether the call panics; use a public value, or read the \
                 secret with `.expose()`",
                value.span,
            );
        }
        Ty::Error
    }

    /// Report `Secret.new`, called at `span`, making the secret `ty` when
    /// that is not a type code may make a secret of: a `Secret<String>`
    /// comes only from `Env.secret` (`E0271`), and the other secrets are
    /// those of [`SECRET_TYPES`] (`E0223`).
    pub(super) fn secret_made(&mut self, ty: &Ty, span: Span) {
        let Some(inner) = self.secret_inner(ty) else {
            return;
        };
        let inner = self.infer.resolve(&inner);
        if inner.is_error() || matches!(inner, Ty::Var(_) | Ty::Never) {
            return;
        }
        if inner.head() == Some(ty::STRING) {
            self.error(
                codes::FORGED,
                "a `Secret<String>` cannot be made from text in the program: it comes from \
                 `Env.secret`, so a secret in the source is never mistaken for one from the \
                 environment",
                span,
            );
            return;
        }
        if inner.is_float() {
            self.error(codes::SECRET_FLOAT, SECRET_FLOAT, span);
            return;
        }
        if inner.head().is_some_and(|h| SECRET_TYPES.contains(&h)) {
            return;
        }
        self.error(
            codes::BAD_OPERAND,
            format!(
                "`Secret.new` cannot make a `Secret<{inner}>`; it makes `Secret<U8>`, \
                 `Secret<U32>`, `Secret<U64>`, `Secret<Bool>` and `Secret<Bytes>`"
            ),
            span,
        );
    }

    /// Report `ct_select` at `span` picking between secrets of type `ty`,
    /// when that is not a type it can pick between without a branch.
    pub(super) fn ct_select_type(&mut self, ty: &Ty, span: Span) {
        let Some(inner) = self.secret_inner(ty) else {
            return;
        };
        let inner = self.infer.resolve(&inner);
        if inner.is_error() || matches!(inner, Ty::Var(_) | Ty::Never) {
            return;
        }
        if inner.head().is_some_and(|h| SECRET_TYPES.contains(&h)) {
            return;
        }
        self.error(
            codes::BAD_OPERAND,
            format!(
                "`ct_select` cannot pick between two `Secret<{inner}>` without a branch; it \
                 picks between secret integers, `Secret<Bool>` and `Secret<Bytes>`"
            ),
            span,
        );
    }
}
