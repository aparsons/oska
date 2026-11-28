// SPDX-License-Identifier: Apache-2.0
//! Traits and `?`.
//!
//! # Trait calls
//!
//! A trait's method called on a value of a type parameter
//! ([`Callee::Trait`]) is resolved statically: every instance is lowered
//! with concrete types, so the receiver's type names the impl, as its
//! runtime type tag does in the interpreter ([`Body::resolve`]). The
//! program's impl `<Tag as Trait>.method` is called when it has one, with
//! the impl's generics bound by matching its `Self` type against the
//! receiver's, then the method's own; otherwise the prelude's intrinsic
//! `Tag.method`.
//!
//! # Equality
//!
//! The prelude's `Eq` impls ([`Body::equal`]) compare words (integers,
//! `Bool`, `()`), text (`String`, `Path`, `IoError`'s message), or, for
//! `Option` and `Result`, the variant and then each field through `Eq` for
//! its own type, which may be the program's impl: a function generated
//! per type ([`Instance::Equal`]). Lists compare their lengths, then
//! their items in order, by a function generated per item type
//! ([`Instance::ListEq`], `lists.rs`).
//!
//! # `?`
//!
//! `operand?` reads the variant: `Some(x)` and `Ok(x)` give `x`; `None`
//! and, without a conversion, `Err(e)` are returned as they are (an
//! `Option` or `Result` object's layout does not depend on its type
//! arguments); with a `From` conversion a new `Err` holds the converted
//! error. An early return releases everything the function holds, the
//! values pending in the expression around the `?` included.

use oska_check::builtins;
use oska_check::hashing;
use oska_check::program::{trait_method, Callee, Expr, Function, Place, Type};
use oska_syntax::{Diagnostic, Span};

use super::{Body, Instance};
use crate::ir::{BinOp, Cond, FloatCond, Inst, Temp};
use crate::layout::{self, Fields};
use crate::NOWHERE;

/// The types whose `Eq` impl is the prelude's, by type tag.
const PRELUDE_EQ: &[&str] = &[
    "()", "Bool", "Int", "Float", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "String",
    "Path", "IoError", "Option", "Result", "List", "Bytes",
];

/// The types whose `Hash` impl is the prelude's, by type tag.
const PRELUDE_HASH: &[&str] = &[
    "Bool", "Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "String", "Path",
    "Option", "List", "Bytes",
];

/// The intrinsic a struct or enum that derives `Hash` is hashed by.
const DERIVED_HASH: &str = "(derived).hash";

/// The interpreter's type tag of values of the concrete type `ty`, which
/// names the impls that apply to them.
fn type_tag(ty: &Type) -> Option<&str> {
    match ty {
        Type::Unit => Some("()"),
        Type::Bool => Some(builtins::BOOL),
        Type::Int(int) => Some(int.name()),
        Type::Float => Some(builtins::FLOAT),
        Type::String => Some(builtins::STRING),
        Type::Named { name, .. } => Some(name),
        _ => None,
    }
}

/// The type arguments of `function`, an impl's method, for a receiver of
/// the concrete type `ty`: the impl's generics bound by matching its
/// `Self` type against `ty`, then `own`, the method's own.
fn impl_type_args(function: &Function, ty: &Type, own: Vec<Type>) -> Vec<Type> {
    let mut bound = Vec::new();
    if let Some(self_ty) = &function.self_ty {
        self_ty.bind(ty, &mut bound);
    }
    let impl_generics = function.generics.len().saturating_sub(own.len());
    let mut args: Vec<Type> = function.generics[..impl_generics]
        .iter()
        .map(|g| {
            bound
                .iter()
                .find(|(name, _)| name == g)
                .map_or(Type::Unit, |(_, t)| t.clone())
        })
        .collect();
    args.extend(own);
    args
}

impl Body<'_, '_> {
    /// What `callee`, instantiated with `type_args`, reaches when its
    /// receiver (first argument) is of `receiver` (not yet substituted):
    /// a trait call resolved to the impl for the receiver's type, with
    /// concrete type arguments. Any other callee is already resolved.
    pub(super) fn resolve(
        &self,
        callee: &Callee,
        type_args: &[Type],
        receiver: Option<&Type>,
        span: Span,
    ) -> Result<(Callee, Vec<Type>), Diagnostic> {
        let own: Vec<Type> = type_args.iter().map(|t| self.ty(t)).collect();
        let Callee::Trait { trait_name, method } = callee else {
            return Ok((callee.clone(), own));
        };
        let ty = receiver.map(|t| self.ty(t)).unwrap_or(Type::Unit);
        let Some(tag) = type_tag(&ty) else {
            return Err(self.refuse(
                span,
                &format!("a call of `{trait_name}.{method}` on a `{ty}`"),
            ));
        };
        let name = trait_method(tag, trait_name, method);
        Ok(match self.program().functions.get(&name) {
            Some(function) => {
                let type_args = impl_type_args(function, &ty, own);
                (Callee::Function(name), type_args)
            }
            // A struct or enum without an `impl Hash` derives it.
            None if *trait_name == builtins::HASH && !PRELUDE_HASH.contains(&tag) => {
                (Callee::intrinsic(DERIVED_HASH), Vec::new())
            }
            None => (Callee::intrinsic(format!("{tag}.{method}")), Vec::new()),
        })
    }

    /// The type of the value at `place`, concrete.
    pub(super) fn place_type(&self, place: &Place) -> Option<Type> {
        match place {
            Place::Local(slot) => self.slots.get(*slot).map(|s| s.ty.clone()),
            Place::SelfValue => self.receiver.as_ref().map(|r| r.ty.clone()),
            Place::Field { base, index } => match self.fields_of(&self.place_type(base)?)? {
                Fields::Struct(fields) => fields.get(*index).cloned(),
                Fields::Enum(_) => None,
            },
        }
    }

    // ---- Equality ---------------------------------------------------------

    /// A prelude `Eq` impl's `eq`, `name` (`Tag.eq`), applied to `values`,
    /// evaluated from `args`: whether they are equal, both released.
    /// `None` for any other intrinsic.
    pub(super) fn eq_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let Some(tag) = name.strip_suffix(".eq") else {
            return Ok(None);
        };
        let (true, [lhs, _], &[a, b]) = (PRELUDE_EQ.contains(&tag), args, values) else {
            return Ok(None);
        };
        let ty = self.ty(&lhs.ty);
        let equal = self.equal(&ty, a, b, span)?;
        self.consume(a)?;
        self.consume(b)?;
        Ok(Some(equal))
    }

    /// Whether `a` and `b`, borrowed values of the concrete type `ty`, are
    /// equal through `Eq` for `ty`: 1 or 0.
    pub(super) fn equal(
        &mut self,
        ty: &Type,
        a: Temp,
        b: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let Some(tag) = type_tag(ty) else {
            return Err(self.refuse(span, &format!("comparing values of `{ty}`")));
        };
        let user = trait_method(tag, builtins::EQ, "eq");
        if let Some(function) = self.program().functions.get(&user) {
            let type_args = impl_type_args(function, ty, Vec::new());
            return self.call_program(&user, type_args, vec![a, b], span);
        }
        match ty {
            Type::Unit => Ok(self.constant(1)),
            Type::Bool | Type::Int(_) => {
                let dst = self.temp();
                self.push(Inst::Compare {
                    cond: Cond::Eq,
                    dst,
                    lhs: a,
                    rhs: b,
                });
                Ok(dst)
            }
            Type::Float => Ok(self.float_compare(FloatCond::Eq, a, b)),
            Type::String => self.call_runtime("string_eq", vec![a, b], NOWHERE),
            Type::Named { name, .. } if name == builtins::PATH => {
                self.call_runtime("path_eq", vec![a, b], NOWHERE)
            }
            Type::Named { name, .. } if name == builtins::BYTES => {
                self.call_runtime("bytes_eq", vec![a, b], NOWHERE)
            }
            Type::Named { name, .. } if name == builtins::IO_ERROR => {
                let a = self.load(a, layout::struct_field(0));
                let b = self.load(b, layout::struct_field(0));
                self.call_runtime("string_eq", vec![a, b], NOWHERE)
            }
            Type::Named { name, args } if name == builtins::LIST => {
                let item = args.first().cloned().unwrap_or(Type::Unit);
                self.list_equal(&item, a, b, span)
            }
            Type::Named { name, .. } if name == builtins::OPTION || name == builtins::RESULT => {
                let equal = Instance::Equal(self.source, ty.clone(), self.step_site(span));
                let id = self.lowering.request(equal, self.origin.unwrap_or(span));
                self.call_id(id, vec![a, b], span)
            }
            _ => Err(self.refuse(span, &format!("`{tag}.eq`"))),
        }
    }

    /// The body of [`Instance::Equal`] for the enum type `ty` (`Option` or
    /// `Result`): its parameters hold the same variant, with each field
    /// equal to the other's through `Eq` for the field's type.
    pub(super) fn equal_function(&mut self, ty: &Type, span: Span) -> Result<(), Diagnostic> {
        let (a, b) = (self.temp(), self.temp());
        let Some(Fields::Enum(variants)) = self.fields_of(ty) else {
            return Err(self.refuse(span, &format!("comparing values of `{ty}`")));
        };
        let result = self.constant(0);
        let end = self.label();
        let tag = self.load(a, layout::ENUM_TAG);
        let other = self.load(b, layout::ENUM_TAG);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: tag,
            rhs: other,
            target: end,
        });
        for (index, fields) in variants.iter().enumerate() {
            if fields.is_empty() {
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
                let x = self.load(a, layout::enum_field(at));
                let y = self.load(b, layout::enum_field(at));
                let same = self.equal(field, x, y, span)?;
                self.push(Inst::JumpIfZero {
                    cond: same,
                    target: end,
                });
            }
            self.push(Inst::Const {
                dst: result,
                value: 1,
            });
            self.push(Inst::Jump(end));
            self.push(Inst::Label(next));
        }
        // A variant without fields.
        self.push(Inst::Const {
            dst: result,
            value: 1,
        });
        self.push(Inst::Label(end));
        self.push(Inst::Return(result));
        Ok(())
    }

    // ---- Order ------------------------------------------------------------

    /// `cmp(self, other)` of an `Option` or a `List`, called as the
    /// intrinsic `name`, evaluated from `args`: an `Ordering`, both
    /// receivers released. `None` for any other intrinsic.
    pub(super) fn cmp_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let Some(tag) = name.strip_suffix(".cmp") else {
            return Ok(None);
        };
        let (true, [lhs, _], &[a, b]) = (matches!(tag, "Option" | "List"), args, values) else {
            return Ok(None);
        };
        let ty = self.ty(&lhs.ty);
        let order = self.compare(&ty, a, b, span)?;
        self.consume(a)?;
        self.consume(b)?;
        // `Ordering` is a fieldless enum: its variant's index, `Less` first.
        let one = self.constant(1);
        Ok(Some(self.binary_op(BinOp::Add, order, one)))
    }

    /// How `a` orders against `b`, borrowed values of the concrete type
    /// `ty`, through `Ord` for `ty`: -1, 0 or 1.
    pub(super) fn compare(
        &mut self,
        ty: &Type,
        a: Temp,
        b: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let Some(tag) = type_tag(ty) else {
            return Err(self.refuse(span, &format!("ordering values of `{ty}`")));
        };
        let user = trait_method(tag, builtins::ORD, "cmp");
        if let Some(function) = self.program().functions.get(&user) {
            let type_args = impl_type_args(function, ty, Vec::new());
            let ordering = self.call_program(&user, type_args, vec![a, b], span)?;
            let one = self.constant(1);
            return Ok(self.binary_op(BinOp::Sub, ordering, one));
        }
        match ty {
            Type::Bool | Type::Int(_) => {
                let unsigned = matches!(ty, Type::Int(int) if int.min() == 0);
                let (below, above) = if unsigned {
                    (Cond::Below, Cond::Above)
                } else {
                    (Cond::Lt, Cond::Gt)
                };
                let (lt, gt) = (self.temp(), self.temp());
                self.push(Inst::Compare {
                    cond: below,
                    dst: lt,
                    lhs: a,
                    rhs: b,
                });
                self.push(Inst::Compare {
                    cond: above,
                    dst: gt,
                    lhs: a,
                    rhs: b,
                });
                Ok(self.binary_op(BinOp::Sub, gt, lt))
            }
            Type::String => self.call_runtime("string_compare", vec![a, b], NOWHERE),
            Type::Named { name, .. } if name == builtins::OPTION || name == builtins::LIST => {
                let compare = Instance::Compare(self.source, ty.clone(), self.step_site(span));
                let id = self.lowering.request(compare, self.origin.unwrap_or(span));
                self.call_id(id, vec![a, b], span)
            }
            _ => Err(self.refuse(span, &format!("`{tag}.cmp`"))),
        }
    }

    /// The body of [`Instance::Compare`] for the `Option` or `List` type
    /// `ty`: -1, 0 or 1 for its two parameters. `None` is before `Some`;
    /// lists compare item by item, and a prefix is before a longer list.
    pub(super) fn compare_function(&mut self, ty: &Type, span: Span) -> Result<(), Diagnostic> {
        let (a, b) = (self.temp(), self.temp());
        match ty {
            Type::Named { name, args } if name == builtins::LIST => {
                let item = args.first().cloned().unwrap_or(Type::Unit);
                self.compare_lists(&item, a, b, span)
            }
            _ => self.compare_options(ty, a, b, span),
        }
    }

    /// Lists `a` and `b` of `item`s: the first items that differ order
    /// them, else the lengths do.
    fn compare_lists(
        &mut self,
        item: &Type,
        a: Temp,
        b: Temp,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let length = self.load(a, layout::LIST_LEN);
        let other = self.load(b, layout::LIST_LEN);
        // The shared length, then the first items that differ.
        let shared = self.copy(length);
        let end = self.label();
        self.push(Inst::Branch {
            cond: Cond::Le,
            lhs: length,
            rhs: other,
            target: end,
        });
        self.push(Inst::Copy {
            dst: shared,
            src: other,
        });
        self.push(Inst::Label(end));
        let zero = self.constant(0);
        let at = self.copy(zero);
        let (head, done) = (self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: shared,
            target: done,
        });
        let x = self.item(a, at);
        let y = self.item(b, at);
        let order = self.compare(item, x, y, span)?;
        let next = self.label();
        self.push(Inst::JumpIfZero {
            cond: order,
            target: next,
        });
        self.push(Inst::Return(order));
        self.push(Inst::Label(next));
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(done));
        let (below, above) = (self.temp(), self.temp());
        self.push(Inst::Compare {
            cond: Cond::Lt,
            dst: below,
            lhs: length,
            rhs: other,
        });
        self.push(Inst::Compare {
            cond: Cond::Gt,
            dst: above,
            lhs: length,
            rhs: other,
        });
        let order = self.binary_op(BinOp::Sub, above, below);
        self.push(Inst::Return(order));
        Ok(())
    }

    /// `Option`s `a` and `b` of the type `ty`.
    fn compare_options(
        &mut self,
        ty: &Type,
        a: Temp,
        b: Temp,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(Fields::Enum(variants)) = self.fields_of(ty) else {
            return Err(self.refuse(span, &format!("ordering values of `{ty}`")));
        };
        // `Option`: the variant without a field is `None`.
        let Some(none) = variants.iter().position(Vec::is_empty) else {
            return Err(self.refuse(span, &format!("ordering values of `{ty}`")));
        };
        let none = self.constant(none as i64);
        let tag = self.load(a, layout::ENUM_TAG);
        let other = self.load(b, layout::ENUM_TAG);
        let same = self.label();
        self.push(Inst::Branch {
            cond: Cond::Eq,
            lhs: tag,
            rhs: other,
            target: same,
        });
        // The tags differ: the one that is `None` is first.
        let (is_none, is_some) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Eq,
            lhs: tag,
            rhs: none,
            target: is_none,
        });
        let greater = self.constant(1);
        self.push(Inst::Return(greater));
        self.push(Inst::Label(is_none));
        let less = self.constant(-1);
        self.push(Inst::Return(less));
        self.push(Inst::Label(same));
        // The same variant: two `None`s are equal.
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: tag,
            rhs: none,
            target: is_some,
        });
        let equal = self.constant(0);
        self.push(Inst::Return(equal));
        self.push(Inst::Label(is_some));
        let field = variants
            .iter()
            .find_map(|fields| fields.first())
            .cloned()
            .unwrap_or(Type::Unit);
        let x = self.load(a, layout::enum_field(0));
        let y = self.load(b, layout::enum_field(0));
        let order = self.compare(&field, x, y, span)?;
        self.push(Inst::Return(order));
        Ok(())
    }

    // ---- Hash -------------------------------------------------------------

    /// `hash(self)` of an `Option`, a `List`, or a struct or enum that
    /// derives `Hash`, called as the intrinsic `name`, evaluated from
    /// `args`: the hash, with the receiver released. `None` for any other
    /// intrinsic.
    pub(super) fn hash_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        let Some(tag) = name.strip_suffix(".hash") else {
            return Ok(None);
        };
        let (true, [receiver], &[a]) = (
            matches!(tag, "Option" | "List") || name == DERIVED_HASH,
            args,
            values,
        ) else {
            return Ok(None);
        };
        let ty = self.ty(&receiver.ty);
        let hash = self.hash_of(&ty, a, span)?;
        self.consume(a)?;
        Ok(Some(hash))
    }

    /// The hash of `a`, a borrowed value of the concrete type `ty`,
    /// through `Hash` for `ty`: the program's impl, the prelude's, or the
    /// derived one.
    pub(super) fn hash_of(&mut self, ty: &Type, a: Temp, span: Span) -> Result<Temp, Diagnostic> {
        let Some(tag) = type_tag(ty) else {
            return Err(self.refuse(span, &format!("hashing values of `{ty}`")));
        };
        let user = trait_method(tag, builtins::HASH, "hash");
        if let Some(function) = self.program().functions.get(&user) {
            let type_args = impl_type_args(function, ty, Vec::new());
            return self.call_program(&user, type_args, vec![a], span);
        }
        match ty {
            Type::Bool | Type::Int(_) => self.call_runtime("int_hash", vec![a], NOWHERE),
            Type::String => self.call_runtime("string_hash", vec![a], NOWHERE),
            Type::Named { name, .. } if name == builtins::PATH => {
                self.call_runtime("path_hash", vec![a], NOWHERE)
            }
            Type::Named { name, .. } if name == builtins::BYTES => {
                self.call_runtime("bytes_hash", vec![a], NOWHERE)
            }
            Type::Named { .. } => {
                let hash = Instance::Hash(self.source, ty.clone(), self.step_site(span));
                let id = self.lowering.request(hash, self.origin.unwrap_or(span));
                self.call_id(id, vec![a], span)
            }
            _ => Err(self.refuse(span, &format!("`{tag}.hash`"))),
        }
    }

    /// The state `state` after it takes the hash of the field or item `a`
    /// of the concrete type `ty`.
    fn hash_mixed(
        &mut self,
        ty: &Type,
        a: Temp,
        state: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let hash = self.hash_of(ty, a, span)?;
        self.call_runtime("hash_mix", vec![state, hash], NOWHERE)
    }

    /// The body of [`Instance::Hash`] for `ty`: the state of the variant's
    /// index (a struct is variant 0, a list's is its length), then the
    /// hash of each field or item in order (`oska_check::hashing`).
    pub(super) fn hash_function(&mut self, ty: &Type, span: Span) -> Result<(), Diagnostic> {
        let a = self.temp();
        let state = self.temp();
        match self.fields_of(ty) {
            Some(Fields::Struct(fields)) => {
                self.push(Inst::Const {
                    dst: state,
                    value: hashing::word(0) as i64,
                });
                for (at, field) in fields.iter().enumerate() {
                    let x = self.load(a, layout::struct_field(at));
                    let next = self.hash_mixed(field, x, state, span)?;
                    self.push(Inst::Copy {
                        dst: state,
                        src: next,
                    });
                }
            }
            Some(Fields::Enum(variants)) if variants.iter().all(Vec::is_empty) => {
                // A fieldless enum is its variant's index.
                let hash = self.call_runtime("int_hash", vec![a], NOWHERE)?;
                self.push(Inst::Copy {
                    dst: state,
                    src: hash,
                });
            }
            Some(Fields::Enum(variants)) => self.hash_variants(&variants, a, state, span)?,
            None => self.hash_list(ty, a, state, span)?,
        }
        self.push(Inst::Return(state));
        Ok(())
    }

    /// Mix the tag and the fields of the enum `a` with `variants` into
    /// `state`.
    fn hash_variants(
        &mut self,
        variants: &[Vec<Type>],
        a: Temp,
        state: Temp,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let tag = self.load(a, layout::ENUM_TAG);
        let hash = self.call_runtime("int_hash", vec![tag], NOWHERE)?;
        self.push(Inst::Copy {
            dst: state,
            src: hash,
        });
        let end = self.label();
        for (index, fields) in variants.iter().enumerate() {
            if fields.is_empty() {
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
                let x = self.load(a, layout::enum_field(at));
                let mixed = self.hash_mixed(field, x, state, span)?;
                self.push(Inst::Copy {
                    dst: state,
                    src: mixed,
                });
            }
            self.push(Inst::Jump(end));
            self.push(Inst::Label(next));
        }
        self.push(Inst::Label(end));

        Ok(())
    }

    /// Mix the length and the items of the list `a` of type `ty` into
    /// `state`.
    fn hash_list(&mut self, ty: &Type, a: Temp, state: Temp, span: Span) -> Result<(), Diagnostic> {
        let Type::Named { name, args } = ty else {
            return Err(self.refuse(span, &format!("hashing values of `{ty}`")));
        };
        if name != builtins::LIST {
            return Err(self.refuse(span, &format!("hashing values of `{ty}`")));
        }
        let item_ty = args.first().cloned().unwrap_or(Type::Unit);
        let length = self.load(a, layout::LIST_LEN);
        let seed = self.call_runtime("int_hash", vec![length], NOWHERE)?;
        self.push(Inst::Copy {
            dst: state,
            src: seed,
        });
        let zero = self.constant(0);
        let at = self.copy(zero);
        let (head, done) = (self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: done,
        });
        let x = self.item(a, at);
        let mixed = self.hash_mixed(&item_ty, x, state, span)?;
        self.push(Inst::Copy {
            dst: state,
            src: mixed,
        });
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(done));

        Ok(())
    }

    // ---- `?` --------------------------------------------------------------

    /// `operand?`, of type `ty`, converting an error with `convert`,
    /// instantiated with `type_args`, when the checker chose one.
    pub(super) fn try_expr(
        &mut self,
        operand: &Expr,
        convert: Option<&Callee>,
        type_args: &[Type],
        ty: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let operand_ty = self.ty(&operand.ty);
        let option = matches!(&operand_ty, Type::Named { name, .. } if name == builtins::OPTION);
        let (enum_name, success) = if option {
            (builtins::OPTION, builtins::SOME)
        } else {
            (builtins::RESULT, builtins::OK)
        };
        let success = self.variant(enum_name, success, span)?;
        let value = self.expr(operand)?;
        let tag = self.load(value, layout::ENUM_TAG);
        let want = self.constant(success as i64);
        let ok = self.label();
        self.push(Inst::Branch {
            cond: Cond::Eq,
            lhs: tag,
            rhs: want,
            target: ok,
        });
        // Leaving: `value` is not released with what else is held.
        let held = self.owned.remove(&value);
        match convert {
            Some(convert) if !option => {
                let err = self.variant(builtins::RESULT, builtins::ERR, span)?;
                let from = self
                    .variant_field_types(&operand_ty, err)
                    .first()
                    .cloned()
                    .unwrap_or(Type::Unit);
                let ret = self.ret.clone();
                let into = self
                    .variant_field_types(&ret, err)
                    .first()
                    .cloned()
                    .unwrap_or(Type::Unit);
                let error = self.load(value, layout::enum_field(0));
                if self.counted(&from) {
                    self.retain(error);
                    self.own(error, &from);
                }
                if let Some(held_ty) = &held {
                    self.release(value, held_ty)?;
                }
                let (callee, type_args) = self.resolve(convert, type_args, Some(&from), span)?;
                let converted = self.call(&callee, &type_args, vec![error], &into, span)?;
                let object = self.alloc(layout::variant_size(1))?;
                let err = self.constant(err as i64);
                self.store(object, layout::ENUM_TAG, err);
                self.store(object, layout::enum_field(0), converted);
                self.owned.remove(&converted);
                self.return_value(object, span)?;
            }
            // `None`, or `Err` as it is: its reference passes on.
            _ => self.return_value(value, span)?,
        }
        self.push(Inst::Label(ok));
        if let Some(held_ty) = held {
            self.owned.insert(value, held_ty);
        }
        let field = self.load(value, layout::enum_field(0));
        if self.counted(ty) {
            self.retain(field);
            self.own(field, ty);
        }
        self.consume(value)?;
        Ok(field)
    }
}
