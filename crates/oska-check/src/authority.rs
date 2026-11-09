//! Authority: which built-in capabilities a type can reach, whether it is
//! **capability-bearing**, and the report of what each function can reach
//! through its receiver and parameters.
//!
//! [`capability_bearing`] is the one definition of capability-bearing:
//! the `cap` rule on declarations, function purity, closure capture, and
//! contract purity all use it.

use oska_syntax::ast::{FnDecl, ItemKind, Module, Type, TypeKind};

use crate::collect::{DeclKind, Decls};
use crate::items::Lower;
use crate::ty::Ty;
use crate::{FunctionAuthority, Via};

/// What a type can reach.
#[derive(Debug, Clone, Default)]
struct Reach {
    /// Built-in capabilities, in order of first appearance.
    capabilities: Vec<String>,
    /// Capability-bearing. True whenever `capabilities` is non-empty, and
    /// also for a `cap fn`, whose capabilities are not written out.
    bearing: bool,
}

impl Reach {
    fn add(&mut self, other: Reach) {
        for capability in other.capabilities {
            if !self.capabilities.contains(&capability) {
                self.capabilities.push(capability);
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
    reach(ty, decls, &mut Vec::new()).bearing
}

/// What `ty` reaches. Type arguments are expanded, and so are the fields
/// of structs and enums, with their generics bound to the arguments.
/// `expanding` holds the types being expanded, so recursive types end.
fn reach(ty: &Ty, decls: &Decls, expanding: &mut Vec<String>) -> Reach {
    match ty {
        Ty::Fn { cap, .. } => Reach {
            capabilities: Vec::new(),
            bearing: *cap,
        },
        Ty::Named(name, args) => {
            let Some(decl) = decls.get(name) else {
                return Reach::default();
            };
            match decl.kind {
                DeclKind::Capability => Reach {
                    capabilities: vec![name.clone()],
                    bearing: true,
                },
                DeclKind::Primitive | DeclKind::Trait => Reach::default(),
                DeclKind::Data => {
                    let mut total = Reach::default();
                    if !expanding.contains(name) {
                        expanding.push(name.clone());
                        let lower = Lower::new(decls).with_generic_names(&decl.generics);
                        let subst: Vec<(String, Ty)> = decl
                            .generics
                            .iter()
                            .map(|g| g.to_string())
                            .zip(args.iter().cloned())
                            .collect();
                        for field in &decl.field_types {
                            let field = lower.ty(field).substitute(&subst);
                            total.add(reach(&field, decls, expanding));
                        }
                        expanding.pop();
                    }
                    for arg in args {
                        total.add(reach(arg, decls, expanding));
                    }
                    total
                }
            }
        }
        Ty::Unit | Ty::Param(_) | Ty::Var(_) | Ty::Never | Ty::Error => Reach::default(),
    }
}

/// One entry per function and `impl` method, in source order. A method
/// is named `Type.method`.
pub fn report(module: &Module, decls: &Decls) -> Vec<FunctionAuthority> {
    let mut functions = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                functions.push(function(f.name.name.clone(), f, Lower::new(decls)));
            }
            ItemKind::Impl(imp) => {
                let lower = Lower::new(decls).with_generics(&imp.generics);
                let target = lower.ty(&imp.target);
                let lower = lower.with_self(target);
                let type_name = type_name(&imp.target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        let name = format!("{type_name}.{}", f.name.name);
                        functions.push(function(name, f, lower.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    functions
}

/// `lower` has the enclosing impl's generics and `Self` in scope.
fn function(name: String, f: &FnDecl, lower: Lower) -> FunctionAuthority {
    let decls = lower.decls();
    let lower = lower.with_generics(&f.generics);
    let mut sources: Vec<(String, Reach)> = Vec::new();
    if f.receiver.is_some() {
        let receiver = lower
            .self_ty
            .as_ref()
            .map(|ty| reach(ty, decls, &mut Vec::new()))
            .unwrap_or_default();
        sources.push(("self".to_string(), receiver));
    }
    for param in &f.params {
        let r = reach(&lower.ty(&param.ty), decls, &mut Vec::new());
        sources.push((param.name.name.clone(), r));
    }

    let mut total = Reach::default();
    let mut via = Vec::new();
    for (param, r) in sources {
        if r.bearing {
            via.push(Via {
                param,
                capabilities: r.capabilities.clone(),
            });
        }
        total.add(r);
    }
    FunctionAuthority {
        name,
        authority: total.capabilities,
        via,
        pure: !total.bearing,
    }
}

/// The last segment of a type's path, such as `Logger` for
/// `Logger<T>`.
fn type_name(ty: &Type) -> String {
    match &ty.kind {
        TypeKind::Path { segments, .. } => {
            segments.last().map(|s| s.name.clone()).unwrap_or_default()
        }
        TypeKind::Unit => "()".to_string(),
        TypeKind::Fn { .. } => "fn".to_string(),
    }
}
