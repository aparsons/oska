//! Compound values: structs and enums built at run time, field reads,
//! field updates with copy-on-write, mutating methods, and the drop and
//! unshare functions generated per type. `layout.rs` says how the
//! objects are laid out.
//!
//! # Copy-on-write
//!
//! Writing inside a value (`p.x = 1`, or a mutating method changing a
//! field of its receiver) first makes each struct on the way to the field
//! unshared, as the interpreter's `Rc::make_mut` does: a struct object
//! whose count is not 1 (shared, or static) is replaced, where it is
//! held, by a copy of its own ([`Instance::Unshare`]), so every other
//! holder keeps the old value.
//!
//! # Mutating methods
//!
//! A method that may change its receiver ([`writes_self`]) owns it: the
//! caller passes its reference and gets the receiver back, as the method
//! left it, as a second result ([`Inst::ReturnPair`], [`Inst::Second`]).
//! A call on a place writes it back there; a call on any other value
//! releases it. Any other method borrows its receiver, as a function
//! borrows its parameters.

use std::ops::ControlFlow;

use oska_check::builtins;
use oska_check::program::{
    walk, Callee, Construct, Expr, ExprKind, Function, Place, Stmt, Type, Visit,
};
use oska_syntax::{Diagnostic, Span};

use super::{constant_word, Body, Instance, Receiver};
use crate::ir::{BinOp, Cond, Inst, Temp, Width};
use crate::layout::{self, Fields};
use crate::NOWHERE;

/// Whether `function` is a method that may change its receiver: its body
/// assigns to `self` or a field of it, or calls a mutating method on one.
/// Only a `var self` method can.
pub(super) fn writes_self(function: &Function) -> bool {
    function.receiver
        && walk(&function.body, &mut |visit| match visit {
            Visit::Expr(Expr {
                kind: ExprKind::MutatingCall { receiver, .. },
                ..
            }) if in_self(receiver) => ControlFlow::Break(()),
            Visit::Expr(Expr {
                kind: ExprKind::Block { stmts, .. },
                ..
            }) if stmts
                .iter()
                .any(|s| matches!(s, Stmt::Assign { place, .. } if in_self(place))) =>
            {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        })
        .is_break()
}

/// Whether `place` is `self` or inside it.
fn in_self(place: &Place) -> bool {
    match place {
        Place::SelfValue => true,
        Place::Local(_) => false,
        Place::Field { base, .. } => in_self(base),
    }
}

impl Body<'_, '_> {
    /// The fields of objects of the concrete type `ty`.
    pub(super) fn fields_of(&self, ty: &Type) -> Option<Fields> {
        layout::fields(self.program(), ty)
    }

    /// The type of field `index` of the concrete struct type `ty`.
    pub(super) fn struct_field_type(&self, ty: &Type, index: usize) -> Type {
        match self.fields_of(ty) {
            Some(Fields::Struct(fields)) => fields.get(index).cloned().unwrap_or(Type::Unit),
            _ => Type::Unit,
        }
    }

    /// The types of the fields of variant `index` of the concrete enum
    /// type `ty`.
    pub(super) fn variant_field_types(&self, ty: &Type, index: usize) -> Vec<Type> {
        match self.fields_of(ty) {
            Some(Fields::Enum(variants)) => variants.get(index).cloned().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(super) fn store(&mut self, base: Temp, offset: u32, src: Temp) {
        self.push(Inst::Store {
            width: Width::W64,
            base,
            offset,
            src,
        });
    }

    /// Call `std.rt`'s allocator for an object of `size` bytes.
    pub(super) fn alloc(&mut self, size: i64) -> Result<Temp, Diagnostic> {
        let size = self.constant(size);
        self.call_runtime("alloc", vec![size], NOWHERE)
    }

    // ---- Building and reading ---------------------------------------------

    /// A struct or enum value: a fieldless enum is its variant's index; a
    /// value whose fields are all constants is a static object; anything
    /// else is a new object holding its fields' references.
    pub(super) fn construct(
        &mut self,
        expr: &Expr,
        ctor: &Construct,
        fields: &[Expr],
    ) -> Result<Temp, Diagnostic> {
        let program = self.program();
        let Some(decl) = program.types.get(&ctor.ty) else {
            return Err(self.refuse(expr.span, &format!("a `{}` value", ctor.ty)));
        };
        let tag = match &ctor.variant {
            Some(variant) => {
                let index = self.variant(&ctor.ty, variant, expr.span)?;
                if layout::is_fieldless_enum(decl) {
                    return Ok(self.constant(index as i64));
                }
                Some(index as i64)
            }
            None => None,
        };
        let constants: Option<Vec<i64>> =
            fields.iter().map(|f| constant_word(program, f)).collect();
        if let Some(constants) = constants {
            let words: Vec<i64> = tag.into_iter().chain(constants).collect();
            let at = self.lowering.rodata.object(&words);
            return Ok(self.rodata(at));
        }
        let mut values = Vec::with_capacity(fields.len());
        for field in fields {
            values.push(self.expr(field)?);
        }
        let object = match tag {
            Some(tag) => {
                let object = self.alloc(layout::variant_size(values.len()))?;
                let tag = self.constant(tag);
                self.store(object, layout::ENUM_TAG, tag);
                object
            }
            None => self.alloc(layout::struct_size(values.len()))?,
        };
        for (index, &value) in values.iter().enumerate() {
            let offset = match tag {
                Some(_) => layout::enum_field(index),
                None => layout::struct_field(index),
            };
            self.store(object, offset, value);
            // The object takes over the field's reference.
            self.owned.remove(&value);
        }
        self.own(object, &expr.ty);
        Ok(object)
    }

    /// Field `index` of the struct `base`, of type `ty`, holding a
    /// reference of its own when `ty` is counted.
    pub(super) fn field(
        &mut self,
        base: &Expr,
        index: usize,
        ty: &Type,
    ) -> Result<Temp, Diagnostic> {
        let (object, evaluated) = match self.place_value(base) {
            Some(object) => (object, false),
            None => (self.expr(base)?, true),
        };
        let value = self.load(object, layout::struct_field(index));
        if self.counted(ty) {
            self.retain(value);
            self.own(value, ty);
        }
        if evaluated {
            self.consume(object)?;
        }
        Ok(value)
    }

    /// The value of `expr` without a reference of its own, when it is a
    /// local, `self`, or a field path inside one, which nothing can change
    /// while it is read: no count is touched.
    fn place_value(&mut self, expr: &Expr) -> Option<Temp> {
        match &expr.kind {
            ExprKind::Local(slot) => self.slots.get(*slot).map(|s| s.temp),
            ExprKind::SelfValue => self.receiver.as_ref().map(|r| r.temp),
            ExprKind::Field { base, index } => {
                let object = self.place_value(base)?;
                Some(self.load(object, layout::struct_field(*index)))
            }
            _ => None,
        }
    }

    // ---- Writing ----------------------------------------------------------

    /// `place = value`. The place's old value is released.
    pub(super) fn assign(&mut self, place: &Place, value: &Expr) -> Result<(), Diagnostic> {
        let src = self.expr(value)?;
        // The place takes over the value's reference.
        self.owned.remove(&src);
        match place {
            Place::Local(_) | Place::SelfValue => {
                let Some((temp, ty, release)) = self.holder(place) else {
                    return Err(self.refuse(value.span, "assigning to this place"));
                };
                if release {
                    self.release(temp, &ty)?;
                }
                self.push(Inst::Copy { dst: temp, src });
            }
            Place::Field { base, index } => {
                let (object, ty) = self.unique_place(base, value.span)?;
                let field_ty = self.struct_field_type(&ty, *index);
                let offset = layout::struct_field(*index);
                if self.counted(&field_ty) {
                    let old = self.load(object, offset);
                    self.release(old, &field_ty)?;
                }
                self.store(object, offset, src);
            }
        }
        Ok(())
    }

    /// The temporary a local or `self` lives in, its type, and whether it
    /// holds a reference of its own.
    pub(super) fn holder(&self, place: &Place) -> Option<(Temp, Type, bool)> {
        match place {
            Place::Local(slot) => {
                let slot = self.slots.get(*slot)?;
                Some((slot.temp, slot.ty.clone(), slot.release))
            }
            Place::SelfValue => {
                let Receiver { temp, owned, ty } = self.receiver.clone()?;
                let release = owned && self.counted(&ty);
                Some((temp, ty, release))
            }
            Place::Field { .. } => None,
        }
    }

    /// Make the struct at `place` unshared, and every struct on the way to
    /// it: its object, which may now be written, and its type.
    pub(super) fn unique_place(
        &mut self,
        place: &Place,
        span: Span,
    ) -> Result<(Temp, Type), Diagnostic> {
        match place {
            Place::Local(_) | Place::SelfValue => {
                let Some((temp, ty, _)) = self.holder(place) else {
                    return Err(self.refuse(span, "changing this place"));
                };
                self.unshare(temp, &ty, None)?;
                Ok((temp, ty))
            }
            Place::Field { base, index } => {
                let (parent, parent_ty) = self.unique_place(base, span)?;
                let ty = self.struct_field_type(&parent_ty, *index);
                let offset = layout::struct_field(*index);
                let object = self.load(parent, offset);
                self.unshare(object, &ty, Some((parent, offset)))?;
                Ok((object, ty))
            }
        }
    }

    /// Make the struct object in `object`, of the concrete type `ty`,
    /// unshared: when its count is not 1, replace it in `object`, and at
    /// `held_at` (the object and offset holding it) if given, by a copy.
    fn unshare(
        &mut self,
        object: Temp,
        ty: &Type,
        held_at: Option<(Temp, u32)>,
    ) -> Result<(), Diagnostic> {
        if !self.counted(ty) {
            return Ok(());
        }
        let count = self.load(object, 0);
        let one = self.constant(1);
        let done = self.label();
        self.push(Inst::Branch {
            cond: Cond::Eq,
            lhs: count,
            rhs: one,
            target: done,
        });
        let unshare = Instance::Unshare(self.source, ty.clone());
        let id = self
            .lowering
            .request(unshare, self.origin.unwrap_or(NOWHERE));
        let copy = self.call_id(id, vec![object], NOWHERE)?;
        self.push(Inst::Copy {
            dst: object,
            src: copy,
        });
        if let Some((parent, offset)) = held_at {
            self.store(parent, offset, object);
        }
        self.push(Inst::Label(done));
        Ok(())
    }

    /// A call of the mutating method `callee` on `receiver`, a place, with
    /// `args`. A method that may change its receiver takes the place's
    /// reference and gives the receiver back, which goes back to the place.
    pub(super) fn mutating_call(
        &mut self,
        callee: &Callee,
        type_args: &[Type],
        receiver: &Place,
        args: &[Expr],
        ret: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        if matches!(callee, Callee::Intrinsic(name) if name == "List.push") {
            return self.list_push(receiver, args, span);
        }
        let mut values = Vec::with_capacity(args.len() + 1);
        for arg in args {
            values.push(self.expr(arg)?);
        }
        let receiver_ty = self.place_type(receiver);
        let (callee, type_args) = self.resolve(callee, type_args, receiver_ty.as_ref(), span)?;
        let name = match &callee {
            Callee::Function(name) if self.program().functions.contains_key(name) => name,
            Callee::Function(name) | Callee::Intrinsic(name) => {
                return Err(self.refuse(span, &format!("the mutating method `{name}`")))
            }
            Callee::Trait { trait_name, method } => {
                return Err(self.refuse(
                    span,
                    &format!("a call of `{trait_name}.{method}` on a value of a type parameter"),
                ))
            }
        };
        let mutated = self.mutated_receiver(name, &type_args).is_some();
        // Where the receiver is: its temporary, or a field of an unshared
        // struct when the method may change it.
        let (value, held_at) = match receiver {
            Place::Local(_) | Place::SelfValue => match self.holder(receiver) {
                Some((temp, _, _)) => (temp, None),
                None => return Err(self.refuse(span, "calling a mutating method here")),
            },
            Place::Field { base, index } => {
                let offset = layout::struct_field(*index);
                let parent = if mutated {
                    self.unique_place(base, span)?.0
                } else {
                    let Some(parent) = self.place_path(base) else {
                        return Err(self.refuse(span, "calling a mutating method here"));
                    };
                    parent
                };
                (self.load(parent, offset), Some((parent, offset)))
            }
        };
        let mut call_args = vec![value];
        call_args.extend(values.iter().copied());
        let result = self.call_program(name, type_args.clone(), call_args, span)?;
        if mutated {
            let after = self.second_result();
            match held_at {
                Some((parent, offset)) => self.store(parent, offset, after),
                None => self.push(Inst::Copy {
                    dst: value,
                    src: after,
                }),
            }
        }
        for value in values {
            self.consume(value)?;
        }
        self.own(result, ret);
        Ok(result)
    }

    /// The object at `place`, read without touching a count.
    fn place_path(&mut self, place: &Place) -> Option<Temp> {
        match place {
            Place::Local(_) | Place::SelfValue => self.holder(place).map(|(temp, _, _)| temp),
            Place::Field { base, index } => {
                let parent = self.place_path(base)?;
                Some(self.load(parent, layout::struct_field(*index)))
            }
        }
    }

    // ---- Generated functions ----------------------------------------------

    /// The body of [`Instance::Drop`] for `ty`: release each field that
    /// holds a reference (of the object's variant, for an enum), then
    /// free the object, its parameter.
    pub(super) fn drop_function(&mut self, ty: &Type) -> Result<(), Diagnostic> {
        let object = self.temp();
        if let Some(item) = layout::list_item(ty) {
            if self.counted(item) {
                let item = item.clone();
                self.drop_list(object, &item)?;
            }
        }
        match self.fields_of(ty) {
            Some(Fields::Struct(fields)) => {
                for (index, field) in fields.iter().enumerate() {
                    if self.counted(field) {
                        let value = self.load(object, layout::struct_field(index));
                        self.release(value, field)?;
                    }
                }
            }
            Some(Fields::Enum(variants)) => {
                let tag = self.load(object, layout::ENUM_TAG);
                let end = self.label();
                for (index, fields) in variants.iter().enumerate() {
                    if !fields.iter().any(|f| self.counted(f)) {
                        continue;
                    }
                    let next = self.label();
                    let want = self.constant(index as i64);
                    self.push(Inst::Branch {
                        cond: Cond::Ne,
                        lhs: tag,
                        rhs: want,
                        target: next,
                    });
                    for (at, field) in fields.iter().enumerate() {
                        if self.counted(field) {
                            let value = self.load(object, layout::enum_field(at));
                            self.release(value, field)?;
                        }
                    }
                    self.push(Inst::Jump(end));
                    self.push(Inst::Label(next));
                }
                self.push(Inst::Label(end));
            }
            None => {}
        }
        self.call_runtime("free", vec![object], NOWHERE)?;
        let unit = self.constant(0);
        self.push(Inst::Return(unit));
        Ok(())
    }

    /// The body of [`Instance::Unshare`] for the struct type `ty`: a new
    /// object with the fields of the object in its parameter, each
    /// reference counted once more, and that object, shared or static,
    /// counted once less (it stays above zero).
    pub(super) fn unshare_function(&mut self, ty: &Type) -> Result<(), Diagnostic> {
        let old = self.temp();
        let Some(Fields::Struct(fields)) = self.fields_of(ty) else {
            return Err(self.refuse(NOWHERE, &format!("copying a `{ty}` value")));
        };
        let new = self.alloc(layout::struct_size(fields.len()))?;
        for (index, field) in fields.iter().enumerate() {
            let offset = layout::struct_field(index);
            let value = self.load(old, offset);
            if self.counted(field) {
                self.retain(value);
            }
            self.store(new, offset, value);
        }
        let count = self.load(old, 0);
        let zero = self.constant(0);
        let done = self.label();
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: count,
            rhs: zero,
            target: done,
        });
        let one = self.constant(1);
        let less = self.binary_op(BinOp::Sub, count, one);
        self.store(old, 0, less);
        self.push(Inst::Label(done));
        self.push(Inst::Return(new));
        Ok(())
    }

    // ---- `Option` and `Result` --------------------------------------------

    /// The prelude's methods of `Option` and `Result` that only read the
    /// enum, inline: `name` applied to `values`, evaluated from `args`,
    /// for `expr`. `None` for any other intrinsic.
    pub(super) fn data_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        expr: &Expr,
    ) -> Result<Option<Temp>, Diagnostic> {
        let (ty, variant) = match name {
            "Option.is_some" | "Option.unwrap_or" | "Option.ok_or" => {
                (builtins::OPTION, builtins::SOME)
            }
            "Result.is_ok" | "Result.unwrap_or" => (builtins::RESULT, builtins::OK),
            _ => return Ok(None),
        };
        let (Some(&receiver), Some(receiver_expr)) = (values.first(), args.first()) else {
            return Ok(None);
        };
        let wanted = self.variant(ty, variant, expr.span)?;
        let tag = self.load(receiver, layout::ENUM_TAG);
        let wanted = self.constant(wanted as i64);
        let result = match (name, values.get(1)) {
            ("Option.is_some" | "Result.is_ok", _) => {
                let dst = self.temp();
                self.push(Inst::Compare {
                    cond: Cond::Eq,
                    dst,
                    lhs: tag,
                    rhs: wanted,
                });
                dst
            }
            ("Option.unwrap_or" | "Result.unwrap_or", Some(&default)) => {
                self.unwrap_or(receiver, tag, wanted, default, &expr.ty)?
            }
            ("Option.ok_or", Some(&error)) => {
                let receiver_ty = self.ty(&receiver_expr.ty);
                self.ok_or(
                    receiver,
                    tag,
                    wanted,
                    error,
                    &receiver_ty,
                    &expr.ty,
                    expr.span,
                )?
            }
            _ => return Ok(None),
        };
        self.consume(receiver)?;
        Ok(Some(result))
    }

    /// `receiver.unwrap_or(default)`: its field when its variant is
    /// `wanted` (`Some` or `Ok`), else `default`, whose reference passes on.
    fn unwrap_or(
        &mut self,
        receiver: Temp,
        tag: Temp,
        wanted: Temp,
        default: Temp,
        ty: &Type,
    ) -> Result<Temp, Diagnostic> {
        let result = self.temp();
        let (other, end) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: tag,
            rhs: wanted,
            target: other,
        });
        let value = self.load(receiver, layout::enum_field(0));
        if self.counted(ty) {
            self.retain(value);
            if let Some(default_ty) = self.owned.get(&default).cloned() {
                self.release(default, &default_ty)?;
            }
        }
        self.push(Inst::Copy {
            dst: result,
            src: value,
        });
        self.push(Inst::Jump(end));
        self.push(Inst::Label(other));
        self.push(Inst::Copy {
            dst: result,
            src: default,
        });
        self.push(Inst::Label(end));
        self.owned.remove(&default);
        self.own(result, ty);
        Ok(result)
    }

    /// `receiver.ok_or(error)`: `Ok` of its field when it is `Some`, else
    /// `Err(error)`, which takes over `error`'s reference.
    #[allow(clippy::too_many_arguments)]
    fn ok_or(
        &mut self,
        receiver: Temp,
        tag: Temp,
        some: Temp,
        error: Temp,
        receiver_ty: &Type,
        ty: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let ok = self.variant(builtins::RESULT, builtins::OK, span)? as i64;
        let err = self.variant(builtins::RESULT, builtins::ERR, span)? as i64;
        let value_ty = self
            .variant_field_types(receiver_ty, 0)
            .first()
            .cloned()
            .unwrap_or(Type::Unit);
        let object = self.alloc(layout::variant_size(1))?;
        let (none, end) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: tag,
            rhs: some,
            target: none,
        });
        let ok = self.constant(ok);
        self.store(object, layout::ENUM_TAG, ok);
        let value = self.load(receiver, layout::enum_field(0));
        if self.counted(&value_ty) {
            self.retain(value);
        }
        self.store(object, layout::enum_field(0), value);
        if let Some(error_ty) = self.owned.get(&error).cloned() {
            self.release(error, &error_ty)?;
        }
        self.push(Inst::Jump(end));
        self.push(Inst::Label(none));
        let err = self.constant(err);
        self.store(object, layout::ENUM_TAG, err);
        self.store(object, layout::enum_field(0), error);
        self.push(Inst::Label(end));
        self.owned.remove(&error);
        self.own(object, ty);
        Ok(object)
    }
}
