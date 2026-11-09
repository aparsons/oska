//! Collecting declarations: the table of type and trait names in scope,
//! built from the prelude and then the user module.

use std::collections::HashMap;

use oska_syntax::ast::{Fields, Item, ItemKind, Module, Type};
use oska_syntax::{Diagnostic, Span};

use crate::codes;
use crate::prelude;
use crate::ty;

/// What a type-level name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    /// A built-in type such as `Int`.
    Primitive,
    /// A struct marked `#[capability]` in the prelude.
    Capability,
    /// A struct or enum.
    Data,
    Trait,
}

/// A type-level declaration.
#[derive(Debug, Clone)]
pub struct Decl<'a> {
    pub kind: DeclKind,
    /// The names of its generic parameters, in order.
    pub generics: Vec<&'a str>,
    /// The types of all its fields, across every variant of an enum.
    pub field_types: Vec<&'a Type>,
    /// Declared in the prelude (or built in), not in the user module.
    pub in_prelude: bool,
}

/// Every type and trait name in scope. Types and traits share one
/// namespace.
pub struct Decls<'a> {
    names: HashMap<&'a str, Decl<'a>>,
}

impl<'a> Decls<'a> {
    /// Collect the prelude's declarations and then `module`'s, reporting
    /// any name declared twice.
    pub fn collect(module: &'a Module, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut decls = Decls {
            names: HashMap::new(),
        };
        for name in prelude::PRIMITIVES {
            decls.names.insert(
                name,
                Decl {
                    kind: DeclKind::Primitive,
                    generics: Vec::new(),
                    field_types: Vec::new(),
                    in_prelude: true,
                },
            );
        }
        // `Untrusted<String>` is written like a generic type but is the
        // trust flag on text (ADR 0006); lowering turns it into the flag.
        decls.names.insert(
            ty::UNTRUSTED,
            Decl {
                kind: DeclKind::Primitive,
                generics: vec!["T"],
                field_types: Vec::new(),
                in_prelude: true,
            },
        );
        for item in &prelude::module().items {
            if let Some((name, _, decl)) = declaration(item, true) {
                decls.names.insert(name, decl);
            }
        }
        for item in &module.items {
            let Some((name, span, decl)) = declaration(item, false) else {
                continue;
            };
            match decls.names.get(name) {
                Some(previous) => {
                    let place = if previous.in_prelude {
                        "in the prelude"
                    } else {
                        "earlier in this module"
                    };
                    diagnostics.push(Diagnostic::new(
                        codes::DUPLICATE_NAME,
                        format!("`{name}` is already declared {place}"),
                        span,
                    ));
                }
                None => {
                    decls.names.insert(name, decl);
                }
            }
        }
        decls
    }

    pub fn get(&self, name: &str) -> Option<&Decl<'a>> {
        self.names.get(name)
    }

    /// All declared names, for "did you mean" suggestions.
    pub fn names(&self) -> impl Iterator<Item = (&'a str, &Decl<'a>)> + '_ {
        self.names.iter().map(|(name, decl)| (*name, decl))
    }
}

/// The type-level name an item declares, with the span of that name.
fn declaration(item: &Item, in_prelude: bool) -> Option<(&str, Span, Decl<'_>)> {
    let (name, generics, field_types, kind) = match &item.kind {
        ItemKind::Struct(s) => {
            let capability = item.attributes.iter().any(|a| a.name.name == "capability");
            let kind = if capability {
                DeclKind::Capability
            } else {
                DeclKind::Data
            };
            (&s.name, &s.generics, field_types(&s.fields), kind)
        }
        ItemKind::Enum(e) => {
            let types = e.variants.iter().flat_map(|v| field_types(&v.fields));
            (&e.name, &e.generics, types.collect(), DeclKind::Data)
        }
        ItemKind::Trait(t) => (&t.name, &t.generics, Vec::new(), DeclKind::Trait),
        _ => return None,
    };
    let decl = Decl {
        kind,
        generics: generics.iter().map(|g| g.name.name.as_str()).collect(),
        field_types,
        in_prelude,
    };
    Some((name.name.as_str(), name.span, decl))
}

/// The types of `fields`, in order.
pub fn field_types(fields: &Fields) -> Vec<&Type> {
    match fields {
        Fields::Named(fields) | Fields::Positional(fields) => {
            fields.iter().map(|f| &f.ty).collect()
        }
        Fields::Unit => Vec::new(),
    }
}
