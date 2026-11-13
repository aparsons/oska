//! `match`: each arm's pattern tested against the scrutinee, then bound.
//!
//! An arm is lowered in two passes over its pattern. The test
//! ([`Body::test`]) branches to the next arm at the first part that does
//! not match: a literal compared, an enum's variant index, then each
//! field's pattern in order. It reads fields without touching a count,
//! and binds nothing, so a mismatch has nothing to undo. The bind
//! ([`Body::bind`]) then pushes each name's slot, in source order, with a
//! reference of its own. A guard that fails releases them and goes on to
//! the next arm, as the interpreter pops them.
//!
//! The scrutinee is held ([`super::Held`]) while the arms run, so a
//! `return`, `break` or `continue` in an arm releases it, and released
//! once the chosen arm's value is computed.

use oska_check::program::{Arm, Expr, ExprKind, Pattern, Type};
use oska_syntax::Diagnostic;

use super::{Body, Held, Slot};
use crate::ir::{Cond, Inst, Label, Temp};
use crate::layout::{self, Fields};
use crate::NOWHERE;

impl Body<'_, '_> {
    /// `match scrutinee { arms }`.
    pub(super) fn match_arms(
        &mut self,
        scrutinee: &Expr,
        arms: &[Arm],
    ) -> Result<Temp, Diagnostic> {
        let value = self.expr(scrutinee)?;
        let scrutinee_ty = self.ty(&scrutinee.ty);
        let held = self.owned.remove(&value);
        if let Some(held_ty) = &held {
            self.held.push(Held {
                temp: value,
                ty: held_ty.clone(),
                loops: self.loops.len(),
                scope: false,
            });
        }
        let result = self.temp();
        let end = self.label();
        let mut values = Vec::with_capacity(arms.len());
        for arm in arms {
            let next = self.label();
            self.test(&arm.pattern, value, &scrutinee_ty, next)?;
            let height = self.slots.len();
            self.bind(&arm.pattern, value, &scrutinee_ty);
            let failed = match &arm.guard {
                Some(guard) => {
                    let holds = self.expr(guard)?;
                    let failed = self.label();
                    self.push(Inst::JumpIfZero {
                        cond: holds,
                        target: failed,
                    });
                    Some((failed, self.slots[height..].to_vec()))
                }
                None => None,
            };
            let arm_value = self.expr(&arm.body)?;
            self.close_scope(height)?;
            self.push(Inst::Copy {
                dst: result,
                src: arm_value,
            });
            values.push(self.owned.remove(&arm_value));
            self.push(Inst::Jump(end));
            if let Some((failed, bound)) = failed {
                self.push(Inst::Label(failed));
                for slot in bound.iter().rev() {
                    if slot.release {
                        self.release(slot.temp, &slot.ty)?;
                    }
                }
            }
            self.push(Inst::Label(next));
        }
        // The checker guarantees an arm matches: nothing reaches here.
        self.push(Inst::Label(end));
        self.merge_owned(values, result);
        if let Some(held_ty) = held {
            self.held.pop();
            self.release(value, &held_ty)?;
        }
        Ok(result)
    }

    /// Branch to `fail` unless `value`, of the concrete type `ty`, matches
    /// `pattern`. Binds nothing and touches no count.
    fn test(
        &mut self,
        pattern: &Pattern,
        value: Temp,
        ty: &Type,
        fail: Label,
    ) -> Result<(), Diagnostic> {
        match pattern {
            Pattern::Wildcard | Pattern::Bind(_) => Ok(()),
            Pattern::Literal(literal) => self.test_literal(literal, value, ty, fail),
            Pattern::Construct { ctor, fields } => {
                let field_types = match &ctor.variant {
                    None => match layout::fields(self.program(), ty) {
                        Some(Fields::Struct(types)) => types,
                        _ => Vec::new(),
                    },
                    Some(variant) => {
                        let index = self.variant(&ctor.ty, variant, NOWHERE)?;
                        let fieldless = self
                            .program()
                            .types
                            .get(&ctor.ty)
                            .is_some_and(layout::is_fieldless_enum);
                        let wanted = self.constant(index as i64);
                        let tag = if fieldless {
                            value
                        } else {
                            self.load(value, layout::ENUM_TAG)
                        };
                        self.push(Inst::Branch {
                            cond: Cond::Ne,
                            lhs: tag,
                            rhs: wanted,
                            target: fail,
                        });
                        self.variant_field_types(ty, index)
                    }
                };
                for (index, (field, field_ty)) in fields.iter().zip(&field_types).enumerate() {
                    if matches!(field, Pattern::Wildcard | Pattern::Bind(_)) {
                        continue;
                    }
                    let field_value = self.load(value, field_offset(ctor.variant.is_some(), index));
                    self.test(field, field_value, field_ty, fail)?;
                }
                Ok(())
            }
        }
    }

    /// Branch to `fail` unless `value`, of type `ty`, equals `literal`.
    fn test_literal(
        &mut self,
        literal: &Expr,
        value: Temp,
        ty: &Type,
        fail: Label,
    ) -> Result<(), Diagnostic> {
        if matches!(literal.kind, ExprKind::Unit) {
            return Ok(());
        }
        let expected = self.expr(literal)?;
        if *ty == Type::String {
            let equal = self.call_runtime("string_eq", vec![value, expected], NOWHERE)?;
            self.consume(expected)?;
            self.push(Inst::JumpIfZero {
                cond: equal,
                target: fail,
            });
        } else {
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: value,
                rhs: expected,
                target: fail,
            });
        }
        Ok(())
    }

    /// Push a slot for each name `pattern` binds in `value`, of the
    /// concrete type `ty`, which matches it: each holds a reference of its
    /// own.
    fn bind(&mut self, pattern: &Pattern, value: Temp, ty: &Type) {
        match pattern {
            Pattern::Wildcard | Pattern::Literal(_) => {}
            Pattern::Bind(_) => {
                let temp = self.copy(value);
                let release = self.counted(ty);
                if release {
                    self.retain(temp);
                }
                self.slots.push(Slot {
                    temp,
                    release,
                    ty: ty.clone(),
                });
            }
            Pattern::Construct { ctor, fields } => {
                let field_types = match &ctor.variant {
                    None => match layout::fields(self.program(), ty) {
                        Some(Fields::Struct(types)) => types,
                        _ => Vec::new(),
                    },
                    Some(variant) => {
                        let index = self
                            .program()
                            .types
                            .get(&ctor.ty)
                            .and_then(|decl| layout::variant_index(decl, variant))
                            .unwrap_or_default();
                        self.variant_field_types(ty, index)
                    }
                };
                for (index, (field, field_ty)) in fields.iter().zip(&field_types).enumerate() {
                    if !binds(field) {
                        continue;
                    }
                    let field_value = self.load(value, field_offset(ctor.variant.is_some(), index));
                    self.bind(field, field_value, field_ty);
                }
            }
        }
    }
}

/// The offset of field `index` of a struct object, or of an enum
/// object's variant.
fn field_offset(variant: bool, index: usize) -> u32 {
    if variant {
        layout::enum_field(index)
    } else {
        layout::struct_field(index)
    }
}

/// Whether `pattern` binds a name.
fn binds(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Bind(_) => true,
        Pattern::Wildcard | Pattern::Literal(_) => false,
        Pattern::Construct { fields, .. } => fields.iter().any(binds),
    }
}
