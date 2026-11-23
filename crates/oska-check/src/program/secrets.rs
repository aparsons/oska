// SPDX-License-Identifier: Apache-2.0
//! Secrets in the checked program (#224).
//!
//! A `Secret<T>` is the prelude struct `Secret { value: T }` at run time,
//! so a backend's `dbg` and generic code see the wrapper and print
//! `<redacted>`. Each of its operations is the public operation on the
//! value inside, read with [`ExprKind::Field`], with its result wrapped
//! again with [`ExprKind::Construct`].
//!
//! The checker lowers the operators that way (`lower/secrets.rs`): `+ - *`
//! become the `wrapping_` methods, and `& | ^ ~ << >>` the public
//! operators. A call of a secret's method stays a call of the intrinsic,
//! such as `Secret.rotl`, so the program says which secret operation it
//! makes; [`expand`] and [`expand_mutating`] give the public operations it
//! is, for a backend that runs it as them, as the native backend does.
//! `ct_eq` is `==` on the values, and `ct_select`, `and` and `or` are an
//! [`ExprKind::Select`], which evaluates every operand and has no branch
//! to take.

use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use super::{Callee, Construct, Expr, ExprKind, IntType, Place, Type};
use crate::ty;

/// The `T` of the program type `Secret<T>`.
pub fn secret_inner(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named { name, args } if name == ty::SECRET && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

/// `expr`'s value: the value inside it for a secret, `expr` itself for a
/// public operand.
pub fn reveal(expr: Expr) -> Expr {
    match secret_inner(&expr.ty).cloned() {
        Some(inner) => Expr {
            span: expr.span,
            ty: inner,
            kind: ExprKind::Field {
                base: Box::new(expr),
                index: 0,
            },
        },
        None => expr,
    }
}

/// `value` made secret.
pub fn conceal(value: Expr) -> ExprKind {
    ExprKind::Construct {
        ctor: Construct {
            ty: ty::SECRET.to_string(),
            variant: None,
        },
        fields: vec![value],
    }
}

/// A call of the prelude intrinsic `name` on `args`, of type `ty`.
fn intrinsic(name: String, args: Vec<Expr>, ty: Type, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Callee::intrinsic(name),
            type_args: Vec::new(),
            args,
        },
        ty,
        span,
    }
}

/// The name of an integer type that secrets compute on, such as `U32`.
pub fn int_name(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Int(int @ (IntType::U8 | IntType::U32 | IntType::U64)) => Some(int.name()),
        _ => None,
    }
}

/// The call of the secret intrinsic `callee` (such as `Secret.rotl`) on
/// `args`, the receiver first, of type `ty`, as the public operations it
/// is; `None` for one a backend implements itself: `expose`, and
/// `Secret<String>`'s comparisons.
pub fn expand(callee: &str, args: &[Expr], ty: &Type, span: Span) -> Option<ExprKind> {
    let method = callee.strip_prefix("Secret.")?;
    secret_call(method, args.to_vec(), ty, span).ok()
}

/// The call of the secret mutating method `callee` (such as `Secret.push`)
/// on `receiver`, a `Secret<Bytes>`, with `args`, as the mutating method
/// of `Bytes` on the field inside it.
pub fn expand_mutating(callee: &str, receiver: &Place, args: &[Expr]) -> Option<ExprKind> {
    let method = callee.strip_prefix("Secret.")?;
    Some(ExprKind::MutatingCall {
        callee: Callee::intrinsic(format!("Bytes.{method}")),
        type_args: Vec::new(),
        receiver: Place::Field {
            base: Box::new(receiver.clone()),
            index: 0,
        },
        args: args.iter().cloned().map(reveal).collect(),
    })
}

/// The secret method or associated function `method` called on `args`
/// (the receiver first), of type `ty`, lowered; or `args` back when it is
/// an intrinsic of its own, such as `expose`.
fn secret_call(
    method: &str,
    args: Vec<Expr>,
    ty: &Type,
    span: Span,
) -> Result<ExprKind, Vec<Expr>> {
    let receiver_inner = args.first().and_then(|a| secret_inner(&a.ty)).cloned();
    let result_inner = secret_inner(ty).cloned();
    match (method, receiver_inner) {
        ("new", _) => {
            let mut args = args;
            Ok(conceal(args.remove(0)))
        }
        // `Secret<String>.to_bytes` is `String.to_bytes` on the text
        // inside, made secret, as a method of a secret integer is below.
        ("to_bytes", Some(Type::String)) => {
            let text = args.into_iter().map(reveal).collect();
            Ok(conceal(intrinsic(
                "String.to_bytes".to_string(),
                text,
                result_inner.expect("`to_bytes` gives a secret"),
                span,
            )))
        }
        // `Secret<String>`'s comparisons and `expose` stay intrinsics.
        (_, Some(Type::String)) | ("expose", _) => Err(args),
        ("constant_time_eq" | "constant_time_eq_secret", Some(_)) => Ok(ExprKind::Call {
            callee: Callee::intrinsic("Bytes.constant_time_eq".to_string()),
            type_args: Vec::new(),
            args: args.into_iter().map(reveal).collect(),
        }),
        // `Secret<Bytes>.len` is public.
        ("len", Some(inner)) => Ok(ExprKind::Call {
            callee: Callee::intrinsic(format!("{}.len", type_name(&inner))),
            type_args: Vec::new(),
            args: args.into_iter().map(reveal).collect(),
        }),
        (_, Some(inner)) => {
            let Some(result) = result_inner else {
                // A method that mutates a temporary `Secret<Bytes>`.
                return Ok(ExprKind::Call {
                    callee: Callee::intrinsic(format!("{}.{method}", type_name(&inner))),
                    type_args: Vec::new(),
                    args: args.into_iter().map(reveal).collect(),
                });
            };
            let mut args = args.into_iter().map(reveal);
            let value = match method {
                "ct_eq" => {
                    let (a, b) = (
                        args.next().expect("a receiver"),
                        args.next().expect("`other`"),
                    );
                    binary(BinaryOp::Eq, a, b, result, span)
                }
                "xor" => {
                    let (a, b) = (
                        args.next().expect("a receiver"),
                        args.next().expect("`other`"),
                    );
                    binary(BinaryOp::Ne, a, b, result, span)
                }
                "not" => Expr {
                    kind: ExprKind::Not(Box::new(args.next().expect("a receiver"))),
                    ty: result,
                    span,
                },
                "and" | "or" | "ct_select" => {
                    let cond = args.next().expect("a receiver");
                    let other = args.next().expect("an operand");
                    let (if_true, if_false) = match method {
                        "and" => (other, boolean(false, span)),
                        "or" => (boolean(true, span), other),
                        _ => (other, args.next().expect("`if_false`")),
                    };
                    Expr {
                        kind: ExprKind::Select {
                            cond: Box::new(cond),
                            if_true: Box::new(if_true),
                            if_false: Box::new(if_false),
                        },
                        ty: result,
                        span,
                    }
                }
                // The public type's own method: a rotation, a
                // conversion, or a read of `Bytes`.
                _ => intrinsic(
                    format!("{}.{method}", type_name(&inner)),
                    args.collect(),
                    result,
                    span,
                ),
            };
            Ok(conceal(value))
        }
        (_, None) => Err(args),
    }
}

/// The name a type's intrinsics are qualified by, such as `U32` or
/// `Bytes`.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Int(int) => int.name().to_string(),
        Type::Bool => "Bool".to_string(),
        Type::Named { name, .. } => name.clone(),
        other => other.to_string(),
    }
}

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr, ty: Type, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            int: None,
        },
        ty,
        span,
    }
}

fn boolean(value: bool, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Bool(value),
        ty: Type::Bool,
        span,
    }
}
