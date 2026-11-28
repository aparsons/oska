// SPDX-License-Identifier: Apache-2.0
//! Derived `Hash` (ADR 0028): a struct or enum the program declares has
//! `Hash` without an `impl` when every field of every variant has it. The
//! hash is the fixed function of `hashing.rs`, over the variant's index and
//! then each field's hash, so it is the same on every platform and both
//! backends.
//!
//! An `impl Hash` the program writes is used instead. A type that holds a
//! `Secret` does not derive it (`E0354`), and any other part without `Hash`
//! (a `Result`, a capability, a function, later a `Float`) leaves the type
//! without it (`E0265`).

use oska_syntax::ast::Ident;

use super::Body;
use crate::infer::Infer;
use crate::items::FnSig;
use crate::ty::{self, Ty};

impl Body<'_, '_, '_> {
    /// Whether `ty` derives `Hash`: a struct or enum declared outside the
    /// prelude whose fields all implement it, with the generics replaced by
    /// `ty`'s arguments. A type met again while it is being decided counts
    /// as having it, so a recursive type is decided by its other fields.
    pub(super) fn derives_hash(&self, infer: &mut Infer, ty: &Ty, depth: usize) -> bool {
        let Ty::Named(name, args, _) = ty else {
            return false;
        };
        if self.decls().get(name).is_none_or(|d| d.in_prelude) {
            return false;
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
        if self.deriving.borrow().contains(ty) {
            return true;
        }
        self.deriving.borrow_mut().push(ty.clone());
        let subst: Vec<(String, Ty)> = generics.iter().cloned().zip(args.iter().cloned()).collect();
        let fields: Vec<Ty> = shapes
            .into_iter()
            .flat_map(|fields| fields.instantiate(&subst))
            .map(|(_, field)| field)
            .collect();
        let all = fields
            .iter()
            .all(|field| self.implements_in(infer, field, ty::HASH, &[], depth + 1));
        self.deriving.borrow_mut().pop();
        all
    }

    /// Whether the named type `ty` implements `Hash` by derivation and not
    /// through an `impl`.
    pub(super) fn hash_is_derived(&self, ty: &Ty) -> bool {
        let mut trial = self.infer.clone();
        let ty = trial.resolve(ty);
        self.impl_in(&mut trial, &ty, ty::HASH, &[], 0).is_none()
            && self.derives_hash(&mut trial, &ty, 0)
    }

    /// `Hash.hash` as a method of `receiver`, when `name` is `hash` and the
    /// type derives `Hash`.
    pub(super) fn derived_hash_method(&self, receiver: &Ty, name: &Ident) -> Option<FnSig> {
        if name.name != "hash" || !self.hash_is_derived(receiver) {
            return None;
        }
        let info = self.items.traits.get(ty::HASH)?;
        let (sig, _) = info.methods.iter().find(|(sig, _)| sig.name == "hash")?;
        Some(info.method_for(sig, receiver, &[], None))
    }
}
