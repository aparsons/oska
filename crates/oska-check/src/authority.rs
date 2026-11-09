//! The authority report: which built-in capabilities each function can
//! reach through its receiver and parameters.

use std::collections::HashMap;

use oska_syntax::ast::{FnDecl, GenericParam, ItemKind, Module, Type, TypeKind};

use crate::collect::{DeclKind, Decls};
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

/// Type variables in scope, mapped to what they reach.
type Env = HashMap<String, Reach>;

/// One entry per function and `impl` method, in source order. A method
/// is named `Type.method`.
pub fn report(module: &Module, decls: &Decls) -> Vec<FunctionAuthority> {
    let mut functions = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => functions.push(function(f.name.name.clone(), f, &Env::new(), decls)),
            ItemKind::Impl(imp) => {
                // Impl generics reach nothing on their own: a generic type
                // is capability-bearing only once instantiated.
                let mut env = generics_env(&imp.generics);
                let target = reach(&imp.target, &env, decls, &mut Vec::new());
                env.insert("Self".to_string(), target);
                let type_name = type_name(&imp.target);
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        let name = format!("{type_name}.{}", f.name.name);
                        functions.push(function(name, f, &env, decls));
                    }
                }
            }
            _ => {}
        }
    }
    functions
}

fn function(name: String, f: &FnDecl, outer: &Env, decls: &Decls) -> FunctionAuthority {
    let mut env = outer.clone();
    env.extend(generics_env(&f.generics));
    let mut sources: Vec<(String, Reach)> = Vec::new();
    if f.receiver.is_some() {
        let receiver = env.get("Self").cloned().unwrap_or_default();
        sources.push(("self".to_string(), receiver));
    }
    for param in &f.params {
        let r = reach(&param.ty, &env, decls, &mut Vec::new());
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

fn generics_env(generics: &[GenericParam]) -> Env {
    generics
        .iter()
        .map(|g| (g.name.name.clone(), Reach::default()))
        .collect()
}

/// Whether `ty`, as written outside any generic scope, is
/// capability-bearing. Type variables are not.
pub fn bearing(ty: &Type, generics: &[GenericParam], decls: &Decls) -> bool {
    reach(ty, &generics_env(generics), decls, &mut Vec::new()).bearing
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

/// What `ty` reaches. Type arguments are expanded, and so are the fields
/// of structs and enums, with their generics bound to the arguments.
/// `expanding` holds the types being expanded, so recursive types end.
fn reach(ty: &Type, env: &Env, decls: &Decls, expanding: &mut Vec<String>) -> Reach {
    match &ty.kind {
        TypeKind::Unit => Reach::default(),
        TypeKind::Fn { cap, .. } => Reach {
            capabilities: Vec::new(),
            bearing: *cap,
        },
        TypeKind::Path { segments, args } => {
            let [segment] = segments.as_slice() else {
                return Reach::default();
            };
            let name = segment.name.as_str();
            if let Some(bound) = env.get(name) {
                return bound.clone();
            }
            let Some(decl) = decls.get(name) else {
                return Reach::default();
            };
            match decl.kind {
                DeclKind::Capability => Reach {
                    capabilities: vec![name.to_string()],
                    bearing: true,
                },
                DeclKind::Primitive | DeclKind::Trait => Reach::default(),
                DeclKind::Data => {
                    let args: Vec<Reach> = args
                        .iter()
                        .map(|arg| reach(arg, env, decls, expanding))
                        .collect();
                    let mut total = Reach::default();
                    if !expanding.iter().any(|e| e == name) {
                        expanding.push(name.to_string());
                        let inner: Env = decl
                            .generics
                            .iter()
                            .enumerate()
                            .map(|(i, g)| (g.to_string(), args.get(i).cloned().unwrap_or_default()))
                            .collect();
                        for field in &decl.field_types {
                            total.add(reach(field, &inner, decls, expanding));
                        }
                        expanding.pop();
                    }
                    for arg in args {
                        total.add(arg);
                    }
                    total
                }
            }
        }
    }
}
