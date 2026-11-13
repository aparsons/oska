//! Authority: which built-in capabilities a type can reach, whether it is
//! **capability-bearing**, and the report of what each function can reach
//! through its receiver and parameters.
//!
//! [`capability_bearing`] is the one definition of capability-bearing:
//! the `cap` rule on declarations, function purity, closure capture, and
//! contract purity all use it.

use oska_syntax::ast::{FnDecl, ImplDecl, Item, ItemKind, Module, Type, TypeKind};

use crate::collect::{DeclKind, Decls};
use crate::items::{Bound, ImplInfo, Items, Lower};
use crate::link::Modules;
use crate::ty::{Cap, Ty, SELF};
use crate::{FunctionAuthority, Via};

/// What a type can reach.
#[derive(Debug, Clone, Default)]
struct Reach {
    /// Built-in capabilities, in order of first appearance.
    capabilities: Vec<String>,
    /// Bounded type parameters, in order of first appearance: their type
    /// arguments may be capability-bearing, and the bounds give a body
    /// methods to call on them.
    generics: Vec<String>,
    /// Capability-bearing. True whenever `capabilities` or `generics` is
    /// non-empty, and also for a `cap fn`, whose capabilities are not
    /// written out.
    bearing: bool,
}

impl Reach {
    fn add(&mut self, other: Reach) {
        for capability in other.capabilities {
            if !self.capabilities.contains(&capability) {
                self.capabilities.push(capability);
            }
        }
        for generic in other.generics {
            if !self.generics.contains(&generic) {
                self.generics.push(generic);
            }
        }
        self.bearing |= other.bearing;
    }
}

/// Whether `ty` is **capability-bearing**: it contains a built-in
/// capability or a `cap fn`, through type arguments and the fields of
/// structs and enums. A generic parameter is not: a generic type is
/// capability-bearing only once instantiated. Resolve inference variables
/// first; an unsolved one is not capability-bearing.
pub fn capability_bearing(ty: &Ty, decls: &Decls) -> bool {
    reach(ty, decls, &[], &mut Vec::new()).bearing
}

/// What `ty` reaches. Type arguments are expanded, and so are the fields
/// of structs and enums, with their generics bound to the arguments. A
/// type parameter named in `bounded` reaches itself; any other reaches
/// nothing. `expanding` holds the types being expanded, so recursive types
/// end.
fn reach(ty: &Ty, decls: &Decls, bounded: &[String], expanding: &mut Vec<String>) -> Reach {
    match ty {
        Ty::Fn { cap, .. } => Reach {
            capabilities: Vec::new(),
            generics: Vec::new(),
            bearing: *cap == Cap::Bearing,
        },
        Ty::Named(name, args, _) => {
            let Some(decl) = decls.get(name) else {
                return Reach::default();
            };
            match decl.kind {
                DeclKind::Capability => Reach {
                    capabilities: vec![name.clone()],
                    generics: Vec::new(),
                    bearing: true,
                },
                DeclKind::Primitive | DeclKind::Trait => Reach::default(),
                DeclKind::Data => {
                    let mut total = Reach::default();
                    if !expanding.contains(name) {
                        expanding.push(name.clone());
                        let lower = Lower::in_scope(decls, decl.in_prelude)
                            .with_generic_names(&decl.generics);
                        let subst: Vec<(String, Ty)> = decl
                            .generics
                            .iter()
                            .map(|g| g.to_string())
                            .zip(args.iter().cloned())
                            .collect();
                        for field in &decl.field_types {
                            let field = lower.ty(field).substitute(&subst);
                            total.add(reach(&field, decls, bounded, expanding));
                        }
                        expanding.pop();
                    }
                    for arg in args {
                        total.add(reach(arg, decls, bounded, expanding));
                    }
                    total
                }
            }
        }
        Ty::Param(name) if bounded.contains(name) => Reach {
            capabilities: Vec::new(),
            generics: vec![name.clone()],
            bearing: true,
        },
        Ty::Unit | Ty::Param(_) | Ty::Var(_) | Ty::Never | Ty::Error => Reach::default(),
    }
}

/// One entry per function and `impl` method, in source order, named as
/// [`Modules::function_name`] names it, or [`impl_method_name`] for a
/// method.
pub fn report(
    module: &Module,
    decls: &Decls,
    items: &Items,
    modules: &Modules,
) -> Vec<FunctionAuthority> {
    let mut functions = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                let name = modules.function_name(item.span.start, None, &f.name.name);
                functions.push(function(name, item, f, Lower::new(decls)));
            }
            ItemKind::Impl(imp) => {
                let lower = Lower::new(decls).with_generics(&imp.generics);
                let target = lower.ty(&imp.target);
                let lower = lower.with_self(target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        let name = impl_method_name(modules, items, item, imp, method, f);
                        functions.push(function(name, method, f, lower.clone()));
                    }
                }
            }
            ItemKind::Trait(t) => {
                // A default method is code like any other, so it is
                // reported; a method with no body is only a signature.
                // Its `Self` is bounded by the trait, as in the body
                // checker, so a receiver reaches whatever an impl's does.
                let lower = Lower::new(decls)
                    .with_generics(&t.generics)
                    .with_self(Ty::Param(SELF.to_string()))
                    .with_bound(Bound {
                        param: SELF.to_string(),
                        trait_name: t.name.name.clone(),
                        args: t
                            .generics
                            .iter()
                            .map(|g| Ty::Param(g.name.name.clone()))
                            .collect(),
                    });
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        if f.body.is_some() {
                            let name = modules.function_name(
                                method.span.start,
                                Some(&t.name.name),
                                &f.name.name,
                            );
                            functions.push(function(name, method, f, lower.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    functions
}

/// The name the authority report gives `method`, declared in the `impl`
/// `item`: `Type.method` for an inherent impl and `<Type as
/// Trait>.method` for `impl Trait for Type`, so that the two never share
/// a name (#123). An attenuation's `function` names its method the same
/// way.
pub(crate) fn impl_method_name(
    modules: &Modules,
    items: &Items,
    item: &Item,
    imp: &ImplDecl,
    method: &Item,
    f: &FnDecl,
) -> String {
    let target = type_name(&imp.target);
    let Some(trait_ref) = &imp.trait_ref else {
        return modules.function_name(method.span.start, Some(&target), &f.name.name);
    };
    // The impl's arguments tell two impls of one generic trait apart. An
    // impl the checker could not record has only its trait's name.
    let trait_ref = items
        .impls
        .iter()
        .find(|info| info.span == Some(item.span))
        .map_or_else(|| type_name(trait_ref), ImplInfo::trait_ref);
    modules.trait_method_name(&target, &trait_ref, &f.name.name)
}

/// `lower` has the enclosing impl's generics and `Self` in scope.
fn function(name: String, item: &Item, f: &FnDecl, lower: Lower) -> FunctionAuthority {
    let decls = lower.decls();
    let lower = lower.with_generics(&f.generics);
    let bounded: Vec<String> = lower.bounds.iter().map(|b| b.param.clone()).collect();
    let mut sources: Vec<(String, Reach)> = Vec::new();
    if f.receiver.is_some() {
        let receiver = lower
            .self_ty
            .as_ref()
            .map(|ty| reach(ty, decls, &bounded, &mut Vec::new()))
            .unwrap_or_default();
        sources.push(("self".to_string(), receiver));
    }
    for param in &f.params {
        let r = reach(&lower.ty(&param.ty), decls, &bounded, &mut Vec::new());
        sources.push((param.name.name.clone(), r));
    }

    let mut total = Reach::default();
    let mut via = Vec::new();
    for (param, r) in sources {
        if r.bearing {
            via.push(Via {
                param,
                capabilities: r.capabilities.clone(),
                generics: r.generics.clone(),
            });
        }
        total.add(r);
    }
    FunctionAuthority {
        name,
        public: item.is_public(),
        authority: total.capabilities,
        via,
        pure: !total.bearing,
    }
}

/// The last segment of a type's path, such as `Logger` for
/// `Logger<T>`.
pub(crate) fn type_name(ty: &Type) -> String {
    match &ty.kind {
        TypeKind::Path { segments, .. } => {
            segments.last().map(|s| s.name.clone()).unwrap_or_default()
        }
        TypeKind::Unit => "()".to_string(),
        TypeKind::Fn { .. } => "fn".to_string(),
    }
}
