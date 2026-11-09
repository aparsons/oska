//! Recursive types: a struct or enum must have a base case, a finite
//! value to build. Recursion through `Option`, `List`, or an enum with a
//! non-recursive variant needs no indirection.

use std::collections::HashMap;

use oska_syntax::ast::{Item, ItemKind, Module, Type, TypeKind};
use oska_syntax::Diagnostic;

use crate::codes;
use crate::decls::field_types;
use crate::prelude;

/// How deeply type arguments may nest before a type is assumed to have a
/// base case. Only polymorphic recursion, such as `S<T>` holding
/// `S<List<T>>`, gets this deep.
const MAX_DEPTH: usize = 64;

/// Report every struct and enum in `module` that has no base case.
pub fn module(module: &Module, diagnostics: &mut Vec<Diagnostic>) {
    let mut shapes = Shapes::default();
    for item in prelude::module().items.iter().chain(&module.items) {
        shapes.add(item);
    }
    for item in &module.items {
        let (name, generics) = match &item.kind {
            ItemKind::Struct(s) => (&s.name, &s.generics),
            ItemKind::Enum(e) => (&e.name, &e.generics),
            _ => continue,
        };
        let ty = Ty::Named(name.name.clone(), vec![Ty::Base; generics.len()]);
        if !shapes.has_base_case(&ty, &mut Vec::new()) {
            diagnostics.push(Diagnostic::new(
                codes::NO_BASE_CASE,
                format!(
                    "`{}` contains itself with no base case, so no value of it can be built",
                    name.name
                ),
                name.span,
            ));
        }
    }
}

/// A type with its generic parameters substituted.
#[derive(Debug, Clone)]
enum Ty {
    /// A type that always has a value: a primitive, a capability, a type
    /// parameter, a function, `()`, or a name that does not resolve.
    Base,
    Named(String, Vec<Ty>),
}

/// A struct or enum: its generic parameters, and the field types of each
/// way to build it (one for a struct, one per variant for an enum).
struct Shape<'a> {
    generics: Vec<&'a str>,
    alternatives: Vec<Vec<&'a Type>>,
}

#[derive(Default)]
struct Shapes<'a> {
    shapes: HashMap<&'a str, Shape<'a>>,
}

impl<'a> Shapes<'a> {
    /// Record `item`. The first declaration of a name wins, as in name
    /// resolution.
    fn add(&mut self, item: &'a Item) {
        let (name, generics, alternatives) = match &item.kind {
            ItemKind::Struct(s) => (&s.name, &s.generics, vec![field_types(&s.fields)]),
            ItemKind::Enum(e) => (
                &e.name,
                &e.generics,
                e.variants.iter().map(|v| field_types(&v.fields)).collect(),
            ),
            _ => return,
        };
        self.shapes.entry(name.name.as_str()).or_insert(Shape {
            generics: generics.iter().map(|g| g.name.name.as_str()).collect(),
            alternatives,
        });
    }

    /// Whether a finite value of `ty` exists. `building` holds the types
    /// being built further up, so reaching one again is no base case.
    fn has_base_case(&self, ty: &Ty, building: &mut Vec<String>) -> bool {
        let Ty::Named(name, args) = ty else {
            return true;
        };
        let Some(shape) = self.shapes.get(name.as_str()) else {
            return true;
        };
        let key = format!("{ty:?}");
        if building.contains(&key) {
            return false;
        }
        if building.len() >= MAX_DEPTH {
            return true;
        }
        let env: HashMap<&str, Ty> = shape
            .generics
            .iter()
            .enumerate()
            .map(|(i, g)| (*g, args.get(i).cloned().unwrap_or(Ty::Base)))
            .collect();
        building.push(key);
        let found = shape.alternatives.iter().any(|fields| {
            fields
                .iter()
                .all(|field| self.has_base_case(&self.substitute(field, &env), building))
        });
        building.pop();
        found
    }

    fn substitute(&self, ty: &Type, env: &HashMap<&str, Ty>) -> Ty {
        let TypeKind::Path { segments, args } = &ty.kind else {
            return Ty::Base;
        };
        let [ident] = segments.as_slice() else {
            return Ty::Base;
        };
        let name = ident.name.as_str();
        if let Some(bound) = env.get(name) {
            return bound.clone();
        }
        if !self.shapes.contains_key(name) {
            return Ty::Base;
        }
        let args = args.iter().map(|arg| self.substitute(arg, env)).collect();
        Ty::Named(name.to_string(), args)
    }
}
