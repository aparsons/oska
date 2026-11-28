// SPDX-License-Identifier: Apache-2.0
//! Expressions.

use super::*;

impl Body<'_, '_> {
    pub(super) fn expr(&mut self, expr: &Expr) -> Result<Temp, Diagnostic> {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Unit => Ok(self.constant(0)),
            ExprKind::Bool(value) => Ok(self.constant(i64::from(*value))),
            // The bit pattern: a `U64` above `i64::MAX` wraps to negative.
            ExprKind::Int(value, _) => Ok(self.constant(*value as i64)),
            // The bits of the `Float`, in a word.
            ExprKind::Float(bits) => Ok(self.constant(*bits as i64)),
            ExprKind::FloatBinary { op, lhs, rhs } => self.float_binary(*op, lhs, rhs),
            ExprKind::FloatNeg(operand) => {
                // `-0.0 - x` is `-x` for every `x`, and a NaN made
                // canonical, which flipping the sign bit would not.
                let zero = self.constant(i64::MIN);
                let value = self.expr(operand)?;
                Ok(self.float_op(FloatOp::Sub, zero, value))
            }
            ExprKind::Str(text) => Ok(self.text(text)),
            ExprKind::Local(slot) => match self.slots.get(*slot) {
                Some(&Slot { temp, .. }) => Ok(self.read(temp, &expr.ty)),
                None => Err(self.refuse(span, "a local slot out of scope")),
            },
            ExprKind::SelfValue => match self.receiver.as_ref().map(|r| r.temp) {
                Some(receiver) => Ok(self.read(receiver, &expr.ty)),
                None => Err(self.refuse(span, "`self` outside a method")),
            },
            ExprKind::Block { stmts, value } => {
                let height = self.slots.len();
                for stmt in stmts {
                    self.stmt(stmt)?;
                }
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.close_scope(height)?;
                Ok(value)
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let result = self.temp();
                let (otherwise, end) = (self.label(), self.label());
                self.branch_unless(cond, otherwise)?;
                let then_value = self.expr(then_branch)?;
                self.push(Inst::Copy {
                    dst: result,
                    src: then_value,
                });
                // Not held on the other branch, which may leave the
                // function (`?`) and releases what is held.
                let then_owned = self.owned.remove(&then_value);
                self.push(Inst::Jump(end));
                self.push(Inst::Label(otherwise));
                let else_value = match else_branch {
                    Some(else_branch) => self.expr(else_branch)?,
                    None => self.constant(0),
                };
                self.push(Inst::Copy {
                    dst: result,
                    src: else_value,
                });
                self.push(Inst::Label(end));
                let else_owned = self.owned.remove(&else_value);
                self.merge_owned(vec![then_owned, else_owned], result);
                Ok(result)
            }
            ExprKind::Not(operand) => {
                let value = self.expr(operand)?;
                let one = self.constant(1);
                Ok(self.binary_op(BinOp::Xor, value, one))
            }
            ExprKind::Neg { operand, int } => self.negate(operand, *int, span),
            ExprKind::BitNot { operand, int } => self.bit_not(operand, *int),
            ExprKind::Select {
                cond,
                if_true,
                if_false,
            } => self.select(expr, cond, if_true, if_false),
            ExprKind::Binary { op, lhs, rhs, int } => self.binary(*op, lhs, rhs, *int, span),
            ExprKind::Interpolated(parts) => self.interpolated(parts),
            ExprKind::Construct { ctor, fields } => self.construct(expr, ctor, fields),
            ExprKind::Range { start, end } => {
                // A range is the prelude's struct `Range(start:, end:)`.
                let ctor = Construct {
                    ty: builtins::RANGE.to_string(),
                    variant: None,
                };
                self.construct(expr, &ctor, &[(**start).clone(), (**end).clone()])
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => {
                if let Some(kind) =
                    secret_expansion(callee, |name| secrets::expand(name, args, &expr.ty, span))
                {
                    let kind = secret_bytes_read(kind);
                    return self.expr(&Expr {
                        kind,
                        ty: expr.ty.clone(),
                        span,
                    });
                }
                if let Some(word) = self.borrowed_word(callee, args) {
                    return Ok(word);
                }
                let receiver = args.first().map(|arg| &arg.ty);
                let (callee, type_args) = self.resolve(callee, type_args, receiver, span)?;
                let values = if matches!(&callee, Callee::Intrinsic(name) if borrows_arguments(name))
                {
                    self.borrowed_args(&args.iter().collect::<Vec<_>>())?
                } else {
                    let mut values = Vec::with_capacity(args.len());
                    for arg in args {
                        values.push(self.expr(arg)?);
                    }
                    values
                };
                if let Callee::Intrinsic(name) = &callee {
                    if let Some(value) = self.data_intrinsic(name, args, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.eq_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.hash_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.cmp_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.rotation_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.int_method_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.float_intrinsic(name, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.list_intrinsic(name, args, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.test_intrinsic(name, &type_args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.task_intrinsic(name, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.html_intrinsic(name, &values, expr)? {
                        return Ok(value);
                    }
                    if name == "todo" {
                        let todo = self.panic_static(panics::TODO, panics::NOT_WRITTEN, span)?;
                        self.push(Inst::Jump(todo));
                        // Never reached: a value of no type, holding nothing.
                        return Ok(self.constant(0));
                    }
                    let map_panic = match name.as_str() {
                        "map_entry_limit" => {
                            Some((panics::MAP_ENTRY_LIMIT, panics::MAP_ENTRY_LIMIT_MESSAGE))
                        }
                        "map_probe_limit" => {
                            Some((panics::MAP_PROBE_LIMIT, panics::MAP_PROBE_LIMIT_MESSAGE))
                        }
                        _ => None,
                    };
                    if let Some((code, message)) = map_panic {
                        let label = self.panic_static(code, message, span)?;
                        self.push(Inst::Jump(label));
                        return Ok(self.constant(0));
                    }
                }
                self.call(&callee, &type_args, values, &expr.ty, span)
            }
            ExprKind::Field { base, index } => self.field(base, *index, &expr.ty),
            ExprKind::List(items) => self.list_literal(expr, items),
            ExprKind::Match { scrutinee, arms } => self.match_arms(scrutinee, arms),
            ExprKind::MutatingCall {
                callee,
                type_args,
                receiver,
                args,
            } => match secret_expansion(callee, |name| {
                secrets::expand_mutating(name, receiver, args)
            }) {
                Some(kind) => self.expr(&Expr {
                    kind,
                    ty: expr.ty.clone(),
                    span,
                }),
                None => self.mutating_call(callee, type_args, receiver, args, &expr.ty, span),
            },
            ExprKind::Try {
                operand,
                convert,
                type_args,
            } => self.try_expr(operand, convert.as_ref(), type_args, &expr.ty, span),
            ExprKind::Scope(body) => self.scope_block(body),
            ExprKind::Closure(code) => self.closure(expr, code),
            ExprKind::FnValue { callee, type_args } => self.fn_value(expr, callee, type_args),
            ExprKind::CallValue { callee, args } => self.call_value(callee, args, expr),
            other => Err(self.refuse(span, &describe(other))),
        }
    }

    /// In std.rt, a value's object address (`rt_mem_text(s)` and the
    /// other identity intrinsics that give an `Int`) taken from a slot
    /// that never changes and holds no reference of its own (a
    /// parameter): the word itself, with no retain before and release
    /// after the call, which would cancel out. `None` for any other call.
    pub(super) fn borrowed_word(&mut self, callee: &Callee, args: &[Expr]) -> Option<Temp> {
        let Callee::Function(name) = callee else {
            return None;
        };
        if self.source != Source::Runtime
            || intrinsics::runtime(name) != Some(Runtime::Identity)
            || !name.starts_with("rt_mem_")
            || name.starts_with("rt_mem_as_")
        {
            return None;
        }
        let [arg] = args else {
            return None;
        };
        let ExprKind::Local(slot) = &arg.kind else {
            return None;
        };
        let slot = self.slots.get(*slot)?;
        if slot.release {
            return None;
        }
        let temp = slot.temp;
        Some(self.copy(temp))
    }

    /// The arguments `args` of a call that only reads them while it runs
    /// (see [`borrows_arguments`]), evaluated in order. An argument that
    /// is a local or `self` is borrowed: a copy of its word with no retain
    /// before the call and no release after, which would cancel out. Only
    /// when no argument after it can change the local first (each is a
    /// local, `self` or a literal); otherwise it is read as usual.
    pub(super) fn borrowed_args(&mut self, args: &[&Expr]) -> Result<Vec<Temp>, Diagnostic> {
        let mut values = Vec::with_capacity(args.len());
        for (i, arg) in args.iter().enumerate() {
            let inert_after = args.get(i + 1..).unwrap_or_default().iter().all(|later| {
                matches!(
                    later.kind,
                    ExprKind::Local(_)
                        | ExprKind::SelfValue
                        | ExprKind::Str(_)
                        | ExprKind::Int(..)
                        | ExprKind::Bool(_)
                )
            });
            let borrowed = match &arg.kind {
                _ if !inert_after => None,
                ExprKind::Local(slot) => self.slots.get(*slot).map(|s| s.temp),
                ExprKind::SelfValue => self.receiver.as_ref().map(|r| r.temp),
                _ => None,
            };
            values.push(match borrowed {
                Some(temp) => self.copy(temp),
                None => self.expr(arg)?,
            });
        }
        Ok(values)
    }

    /// The value of the local or receiver in `temp`, of type `ty`: a copy,
    /// so that assigning the local later does not change it, holding a
    /// reference of its own when `ty` is counted.
    pub(super) fn read(&mut self, temp: Temp, ty: &Type) -> Temp {
        let value = self.copy(temp);
        if self.counted(ty) {
            self.retain(value);
            self.own(value, ty);
        }
        value
    }

    /// `result` holds whichever value a branch chose, each taken out of
    /// [`Body::owned`] (`owned`, the type of each that held a reference)
    /// as its branch ended, so that code in a later branch never releases
    /// it: the result holds a reference of its own if any of them did. A
    /// branch may give a static object, which needs no release; releasing
    /// one is harmless.
    pub(super) fn merge_owned(&mut self, owned: Vec<Option<Type>>, result: Temp) {
        if let Some(ty) = owned.into_iter().flatten().last() {
            self.owned.insert(result, ty);
        }
    }
}
