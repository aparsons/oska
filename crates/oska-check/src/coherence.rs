//! Value-level declarations that must be unique, and where methods may be
//! declared:
//!
//! - a free function declared twice, or redeclaring a prelude function,
//!   and a method declared twice for one type across its inherent `impl`
//!   blocks (`E0202`, at the second);
//! - two `impl Trait for Type` that overlap, including one that overlaps a
//!   prelude impl (`E0290`, at the second's trait);
//! - an inherent `impl` for a prelude type (`E0291`): the orphan rule
//!   covers inherent methods too, so user code cannot add a method to
//!   `FsRead` or `Option`.

use std::collections::HashSet;

use oska_syntax::ast::{ImplDecl, Item, ItemKind, Module};
use oska_syntax::Diagnostic;

use crate::codes;
use crate::collect::Decls;
use crate::infer::Infer;
use crate::items::Lower;
use crate::prelude;
use crate::ty::Ty;

pub fn module(module: &Module, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    let mut functions: HashSet<&str> = prelude::module()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(f.name.name.as_str()),
            _ => None,
        })
        .collect();
    let prelude_functions = functions.clone();
    let mut methods: HashSet<(String, &str)> = HashSet::new();
    let mut impls: Vec<TraitImpl> = prelude::module()
        .items
        .iter()
        .filter_map(|item| trait_impl(item, decls, true))
        .collect();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                let name = f.name.name.as_str();
                if !functions.insert(name) {
                    let place = if prelude_functions.contains(name) {
                        "in the prelude"
                    } else {
                        "earlier in this module"
                    };
                    diagnostics.push(Diagnostic::new(
                        codes::DUPLICATE_NAME,
                        format!("the function `{name}` is already declared {place}"),
                        f.name.span,
                    ));
                }
            }
            ItemKind::Impl(imp) if imp.trait_ref.is_none() => {
                inherent(imp, decls, &mut methods, diagnostics);
            }
            ItemKind::Impl(imp) => {
                let Some(new) = trait_impl(item, decls, false) else {
                    continue;
                };
                if let Some(previous) = impls.iter().find(|old| old.overlaps(&new)) {
                    let place = if previous.in_prelude {
                        "the prelude already has"
                    } else {
                        "this module already has"
                    };
                    let trait_ref = imp.trait_ref.as_ref().map_or(item.span, |t| t.span);
                    diagnostics.push(Diagnostic::new(
                        codes::CONFLICTING_IMPL,
                        format!(
                            "conflicting implementations of `{}` for `{}`: {place} one",
                            Ty::generic(&new.trait_name, new.trait_args.clone()),
                            new.target
                        ),
                        trait_ref,
                    ));
                }
                impls.push(new);
            }
            _ => {}
        }
    }
}

/// An inherent `impl`: its type must be the module's own, and no method
/// may be declared twice for one type.
fn inherent<'m>(
    imp: &'m ImplDecl,
    decls: &Decls,
    methods: &mut HashSet<(String, &'m str)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target = Lower::new(decls)
        .with_generics(&imp.generics)
        .ty(&imp.target);
    let Some(type_name) = target.head() else {
        return;
    };
    if decls.get(type_name).is_some_and(|d| d.in_prelude) {
        diagnostics.push(Diagnostic::new(
            codes::FOREIGN_INHERENT_IMPL,
            format!(
                "`{type_name}` is declared in the prelude, so it cannot have an inherent `impl` \
                 here; write a function, or a trait implemented for it"
            ),
            imp.target.span,
        ));
        return;
    }
    for method in &imp.methods {
        let ItemKind::Fn(f) = &method.kind else {
            continue;
        };
        if !methods.insert((type_name.to_string(), f.name.name.as_str())) {
            diagnostics.push(Diagnostic::new(
                codes::DUPLICATE_NAME,
                format!(
                    "`{type_name}` already has a method or associated function `{}`",
                    f.name.name
                ),
                f.name.span,
            ));
        }
    }
}

/// An `impl Trait<Args> for Target`, lowered, with its generic
/// parameters as [`Ty::Param`].
struct TraitImpl {
    generics: Vec<String>,
    trait_name: String,
    trait_args: Vec<Ty>,
    target: Ty,
    in_prelude: bool,
}

/// `item` as a trait impl, when it is one whose trait and target resolve.
fn trait_impl(item: &Item, decls: &Decls, in_prelude: bool) -> Option<TraitImpl> {
    let ItemKind::Impl(imp) = &item.kind else {
        return None;
    };
    let lower = Lower::new(decls).with_generics(&imp.generics);
    let (trait_name, trait_args) = lower.bound(imp.trait_ref.as_ref()?)?;
    let target = lower.ty(&imp.target);
    if target.is_error() {
        return None;
    }
    Some(TraitImpl {
        generics: lower.generics.clone(),
        trait_name,
        trait_args,
        target,
        in_prelude,
    })
}

impl TraitImpl {
    /// Some instantiation of each is the same trait, with the same
    /// arguments, for the same type.
    fn overlaps(&self, other: &TraitImpl) -> bool {
        if self.trait_name != other.trait_name || self.trait_args.len() != other.trait_args.len() {
            return false;
        }
        let mut infer = Infer::default();
        let a = self.instantiate(&mut infer);
        let b = other.instantiate(&mut infer);
        a.iter().zip(&b).all(|(x, y)| infer.unify(x, y))
    }

    /// The target and then the trait's arguments, with each generic
    /// parameter replaced by a fresh variable.
    fn instantiate(&self, infer: &mut Infer) -> Vec<Ty> {
        let subst: Vec<(String, Ty)> = self
            .generics
            .iter()
            .map(|g| (g.clone(), infer.fresh()))
            .collect();
        std::iter::once(&self.target)
            .chain(&self.trait_args)
            .map(|t| t.substitute(&subst))
            .collect()
    }
}
