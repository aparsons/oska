//! Checking `impl Trait for Type`: every required method is provided
//! (`E0261`), each method has the trait's signature with `Self` and the
//! trait's arguments replaced (`E0262`), and no method is one the trait
//! does not declare (`E0263`).

use oska_syntax::ast::{ItemKind, Module};
use oska_syntax::Diagnostic;

use crate::codes;
use crate::collect::Decls;
use crate::items::{FnSig, Items, Lower};
use crate::ty::Ty;

pub fn module(module: &Module, decls: &Decls, items: &Items, diagnostics: &mut Vec<Diagnostic>) {
    for item in &module.items {
        let ItemKind::Impl(imp) = &item.kind else {
            continue;
        };
        let Some(trait_ref) = &imp.trait_ref else {
            continue;
        };
        let lower = Lower::new(decls).with_generics(&imp.generics);
        // An unknown trait or target has already been reported.
        let Some((trait_name, args)) = lower.bound(trait_ref) else {
            continue;
        };
        let Some(info) = items.traits.get(&trait_name) else {
            continue;
        };
        let target = lower.ty(&imp.target);
        if target.is_error() {
            continue;
        }
        let mut provided = Vec::new();
        for method in &imp.methods {
            let ItemKind::Fn(f) = &method.kind else {
                continue;
            };
            provided.push(f.name.name.as_str());
            let Some((declared, _)) = info.methods.iter().find(|(s, _)| s.name == f.name.name)
            else {
                diagnostics.push(Diagnostic::new(
                    codes::NOT_IN_TRAIT,
                    format!(
                        "`{}` is not a method of the trait `{trait_name}`",
                        f.name.name
                    ),
                    f.name.span,
                ));
                continue;
            };
            let actual = lower.signature(f, &[], Some(target.clone()));
            let expected = info.method_for(declared, &target, &args, &[], &[]);
            if !same_signature(&actual, &expected) {
                diagnostics.push(Diagnostic::new(
                    codes::IMPL_SIGNATURE,
                    format!(
                        "`{}` must have the signature `{}` declared by `{trait_name}`",
                        f.name.name,
                        render(&expected)
                    ),
                    f.name.span,
                ));
            }
        }
        let missing: Vec<&str> = info
            .methods
            .iter()
            .filter(|(sig, has_default)| !has_default && !provided.contains(&sig.name.as_str()))
            .map(|(sig, _)| sig.name.as_str())
            .collect();
        if !missing.is_empty() {
            let listed = missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            diagnostics.push(Diagnostic::new(
                codes::MISSING_METHOD,
                format!("this `impl {trait_name}` is missing the required method(s) {listed}"),
                trait_ref.span,
            ));
        }
    }
}

/// Whether an impl's method matches the trait's, with the method's own
/// generic parameters matched by position. A type that failed to lower
/// matches anything: it has already been reported.
fn same_signature(actual: &FnSig, expected: &FnSig) -> bool {
    if actual.receiver != expected.receiver
        || actual.params.len() != expected.params.len()
        || actual.generics.len() != expected.generics.len()
    {
        return false;
    }
    let rename: Vec<(String, Ty)> = expected
        .generics
        .iter()
        .zip(&actual.generics)
        .map(|(e, a)| (e.clone(), Ty::Param(a.clone())))
        .collect();
    actual
        .params
        .iter()
        .zip(&expected.params)
        .all(|((_, a), (_, e))| same_type(a, &e.substitute(&rename)))
        && same_type(&actual.ret, &expected.ret.substitute(&rename))
}

fn same_type(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::Error, _) | (_, Ty::Error) => true,
        (Ty::Named(n1, a1), Ty::Named(n2, a2)) => {
            n1 == n2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(x, y)| same_type(x, y))
        }
        (
            Ty::Fn {
                cap: c1,
                params: p1,
                ret: r1,
            },
            Ty::Fn {
                cap: c2,
                params: p2,
                ret: r2,
            },
        ) => {
            c1 == c2
                && p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(x, y)| same_type(x, y))
                && same_type(r1, r2)
        }
        _ => a == b,
    }
}

/// A signature as it would be written, for messages.
fn render(sig: &FnSig) -> String {
    let mut params = Vec::new();
    match sig.receiver {
        Some(true) => params.push("var self".to_string()),
        Some(false) => params.push("self".to_string()),
        None => {}
    }
    params.extend(sig.params.iter().map(|(n, t)| format!("{n}: {t}")));
    let ret = if sig.ret == Ty::Unit {
        String::new()
    } else {
        format!(" -> {}", sig.ret)
    };
    format!("fn {}({}){ret}", sig.name, params.join(", "))
}
