//! What generic code assumes of its type parameters, checked at every
//! instantiation (`E0274`).
//!
//! Inside a generic body a type parameter is rigid, and never
//! capability-bearing: a contract may call a method on a `T`, and a plain
//! `fn` closure may capture one. Either is only sound if `T` is not
//! instantiated with a capability-bearing type, since the contract would
//! then perform an effect and the plain `fn` would carry authority. In the
//! same way, an unscoped closure that captures a `T` may outlive a `scope`
//! block, so `T` must not hold a `Scope` handle.
//!
//! So a body records each such assumption ([`Assumed`]) under the
//! parameter's [`GenericKey`], and every instantiation of a parameter
//! ([`Instance`]) is checked against the assumptions on it:
//!
//! - a call of a generic function or method instantiates its generics, and
//!   the generics of the impl it belongs to;
//! - satisfying a bound through a generic impl instantiates the impl's
//!   generics;
//! - an impl of a trait instantiates the trait's `Self` and generics;
//! - a generic method of a trait impl stands for the trait's method, so
//!   what its body assumes is assumed of the trait method's parameter.
//!
//! A type argument that is itself a type parameter, or holds one, passes
//! the assumption on to it (`outer<U>` calling `check<T>` with a `U`), so
//! this runs once every body has been checked, to a fixpoint.

use std::collections::{HashMap, HashSet};

use oska_syntax::ast::{GenericParam, ItemKind, Module};
use oska_syntax::{Diagnostic, Span};

use crate::authority::{self, capability_bearing};
use crate::codes;
use crate::collect::Decls;
use crate::items::{GenericKey, Items};
use crate::ty::{Cap, Ty};

/// What a body may assume of a type parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Assumption {
    /// It is not capability-bearing: a contract uses a value of it, or a
    /// plain `fn` closure captures one.
    NotBearing,
    /// It holds no `Scope` handle: an unscoped closure captures a value
    /// of it.
    NoScope,
}

impl Assumption {
    pub const ALL: [Assumption; 2] = [Assumption::NotBearing, Assumption::NoScope];
}

/// A body's assumption about one of its type parameters.
#[derive(Debug, Clone)]
pub struct Assumed {
    pub key: GenericKey,
    pub assumption: Assumption,
    /// Why, for the diagnostic: "a contract in `check` uses a value of
    /// type `T`".
    pub reason: String,
}

/// One instantiation of a type parameter, with what checking it needs.
#[derive(Debug, Clone)]
pub struct Instance {
    pub key: GenericKey,
    /// Where it is instantiated: the callee's name, or an impl's type.
    pub at: Span,
    /// The type argument, as written in a diagnostic.
    pub ty: String,
    /// The type argument is capability-bearing.
    pub bearing: bool,
    /// The type argument holds a `Scope` handle.
    pub holds_scope: bool,
    /// The type parameters, of the code that instantiates, that the type
    /// argument is capability-bearing through when one of them is.
    pub bearing_through: Vec<GenericKey>,
    /// The type parameters it holds a `Scope` handle through.
    pub scope_through: Vec<GenericKey>,
}

/// The type parameters and capability variables in `ty`, resolved, that
/// would make it break `assumption` once instantiated: for
/// [`Assumption::NotBearing`], those reached through type arguments (and so
/// fields), plus the capability flag of a function type; for
/// [`Assumption::NoScope`], also a function type's result.
pub fn positions(ty: &Ty, assumption: Assumption, params: &mut Vec<String>, caps: &mut Vec<u32>) {
    match ty {
        Ty::Param(name) => params.push(name.clone()),
        Ty::Named(_, args, _) => {
            for arg in args {
                positions(arg, assumption, params, caps);
            }
        }
        Ty::Fn { cap, ret, .. } => match assumption {
            Assumption::NotBearing => {
                if let Cap::Var(v) = cap {
                    caps.push(*v);
                }
            }
            Assumption::NoScope => positions(ret, assumption, params, caps),
        },
        Ty::Unit | Ty::Var(_) | Ty::Never | Ty::Error => {}
    }
}

/// The instance of `key` by `ty`, a type written in a declaration, whose
/// type parameters have the keys in `scope`.
fn declared(
    key: GenericKey,
    ty: &Ty,
    at: Span,
    scope: &[(String, Option<GenericKey>)],
    decls: &Decls,
) -> Instance {
    let through = |assumption| {
        let (mut params, mut caps) = (Vec::new(), Vec::new());
        positions(ty, assumption, &mut params, &mut caps);
        params
            .iter()
            .filter_map(|p| scope.iter().find(|(n, _)| n == p).and_then(|(_, k)| *k))
            .collect()
    };
    Instance {
        key,
        at,
        ty: ty.to_string(),
        bearing: capability_bearing(ty, decls),
        holds_scope: ty.holds_scope(),
        bearing_through: through(Assumption::NotBearing),
        scope_through: through(Assumption::NoScope),
    }
}

/// Check every instance against the assumptions, after passing each
/// assumption on through the instances that instantiate one type parameter
/// with another.
pub fn module(
    module: &Module,
    decls: &Decls,
    items: &Items,
    assumed: Vec<Assumed>,
    mut instances: Vec<Instance>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    instances.extend(declared_instances(module, decls, items));
    let mut known: HashMap<(GenericKey, Assumption), String> = HashMap::new();
    for a in assumed {
        known.entry((a.key, a.assumption)).or_insert(a.reason);
    }
    loop {
        let mut added = Vec::new();
        for instance in &instances {
            for assumption in Assumption::ALL {
                let Some(reason) = known.get(&(instance.key, assumption)) else {
                    continue;
                };
                let through = match assumption {
                    Assumption::NotBearing => &instance.bearing_through,
                    Assumption::NoScope => &instance.scope_through,
                };
                for key in through {
                    if !known.contains_key(&(*key, assumption)) {
                        added.push(((*key, assumption), reason.clone()));
                    }
                }
            }
        }
        if added.is_empty() {
            break;
        }
        for (k, reason) in added {
            known.entry(k).or_insert(reason);
        }
    }
    let names = names(module);
    let mut reported = HashSet::new();
    for instance in &instances {
        for assumption in Assumption::ALL {
            let Some(reason) = known.get(&(instance.key, assumption)) else {
                continue;
            };
            let breaks = match assumption {
                Assumption::NotBearing => instance.bearing,
                Assumption::NoScope => instance.holds_scope,
            };
            if !breaks || !reported.insert((instance.at, instance.key, assumption)) {
                continue;
            }
            let ty = &instance.ty;
            let param = names.get(&instance.key).map_or_else(
                || "a type parameter".to_string(),
                |n| format!("the type parameter {n}"),
            );
            let message = match assumption {
                Assumption::NotBearing => format!(
                    "`{ty}` is capability-bearing, so it cannot stand for {param}, which \
                     must be pure: {reason}"
                ),
                Assumption::NoScope => format!(
                    "`{ty}` holds a `Scope` handle, so it cannot stand for {param}, which \
                     must not hold one: {reason}, and could outlive the handle's `scope` block"
                ),
            };
            diagnostics.push(Diagnostic::new(
                codes::BREAKS_TYPE_PARAMETER,
                message,
                instance.at,
            ));
        }
    }
}

/// The instances the declarations make: each impl of a trait the module
/// declares instantiates the trait's `Self` and generics, and each generic
/// parameter of a trait impl's method stands for the trait method's.
fn declared_instances(module: &Module, decls: &Decls, items: &Items) -> Vec<Instance> {
    let mut instances = Vec::new();
    for item in &module.items {
        let ItemKind::Impl(written) = &item.kind else {
            continue;
        };
        let Some(imp) = items.impls.iter().find(|i| i.span == Some(item.span)) else {
            continue;
        };
        let Some(info) = items.traits.get(&imp.trait_name) else {
            continue;
        };
        let scope: Vec<(String, Option<GenericKey>)> = imp
            .generics
            .iter()
            .cloned()
            .zip(imp.generic_keys.iter().copied())
            .collect();
        if let Some(key) = info.self_key {
            let at = written.target.span;
            instances.push(declared(key, &imp.target, at, &scope, decls));
        }
        let at = written.trait_ref.as_ref().map_or(item.span, |t| t.span);
        for (key, arg) in info.generic_keys.iter().zip(&imp.trait_args) {
            if let Some(key) = key {
                instances.push(declared(*key, arg, at, &scope, decls));
            }
        }
        for method in &written.methods {
            let ItemKind::Fn(f) = &method.kind else {
                continue;
            };
            let Some((sig, _)) = info.methods.iter().find(|(s, _)| s.name == f.name.name) else {
                continue;
            };
            for (g, trait_key) in f.generics.iter().zip(&sig.generic_keys) {
                if let Some(trait_key) = trait_key {
                    instances.push(Instance {
                        key: GenericKey(g.name.span),
                        at: g.name.span,
                        ty: g.name.name.clone(),
                        bearing: false,
                        holds_scope: false,
                        bearing_through: vec![*trait_key],
                        scope_through: vec![*trait_key],
                    });
                }
            }
        }
    }
    instances
}

/// How each of the module's type parameters is named in a diagnostic:
/// "`T` of `check`".
fn names(module: &Module) -> HashMap<GenericKey, String> {
    let mut names = HashMap::new();
    let add = |names: &mut HashMap<GenericKey, String>, generics: &[GenericParam], owner: &str| {
        for g in generics {
            names.insert(
                GenericKey(g.name.span),
                format!("`{}` of {owner}", g.name.name),
            );
        }
    };
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => add(&mut names, &f.generics, &format!("`{}`", f.name.name)),
            ItemKind::Impl(imp) => {
                let type_name = authority::type_name(&imp.target);
                add(
                    &mut names,
                    &imp.generics,
                    &format!("the impl for `{type_name}`"),
                );
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        add(
                            &mut names,
                            &f.generics,
                            &format!("`{type_name}.{}`", f.name.name),
                        );
                    }
                }
            }
            ItemKind::Trait(t) => {
                let owner = format!("`{}`", t.name.name);
                add(&mut names, &t.generics, &owner);
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        add(
                            &mut names,
                            &f.generics,
                            &format!("`{}.{}`", t.name.name, f.name.name),
                        );
                    }
                }
                names.insert(GenericKey(t.name.span), format!("`Self` of {owner}"));
            }
            ItemKind::Struct(_)
            | ItemKind::Enum(_)
            | ItemKind::Const(_)
            | ItemKind::Use(_)
            | ItemKind::Test(_) => {}
        }
    }
    names
}
