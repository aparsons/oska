//! Collecting declarations: the table of type and trait names in scope,
//! built from the prelude and then the user module.

use std::collections::HashMap;

use oska_syntax::ast::{Fields, Ident, Item, ItemKind, Module, Type};
use oska_syntax::{Diagnostic, Span};

use crate::codes;
use crate::prelude::{self, StdModule};
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
    /// The `std.*` modules the user module imports, in `use` order.
    imported: Vec<&'static StdModule>,
    /// The modules a plain `use std.x` binds to the name `x`.
    bound: Vec<&'static StdModule>,
    /// Names declared by an imported module that no brace group imports:
    /// the user module reaches them only qualified, as in `sql.Db`.
    qualified_only: HashMap<&'static str, &'static StdModule>,
}

impl<'a> Decls<'a> {
    /// Collect the prelude's declarations, then those of every `std.*`
    /// module `module` imports, then `module`'s, reporting any name
    /// declared twice.
    pub fn collect(module: &'a Module, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut imported: Vec<&'static StdModule> = Vec::new();
        let mut bound: Vec<&'static StdModule> = Vec::new();
        let mut unqualified: Vec<&str> = Vec::new();
        for item in &module.items {
            if let ItemKind::Use(u) = &item.kind {
                let path: Vec<&str> = u.path.iter().map(|s| s.name.as_str()).collect();
                // An unknown module is reported by name resolution.
                let Some(std) = prelude::std_module(&path.join(".")) else {
                    continue;
                };
                if !imported.iter().any(|m| m.path == std.path) {
                    imported.push(std);
                }
                match &u.names {
                    // Only exports can be imported; others stay hidden.
                    Some(names) => unqualified.extend(
                        names
                            .iter()
                            .map(|n| n.name.as_str())
                            .filter(|n| std.exports().contains(n)),
                    ),
                    None => {
                        if !bound.iter().any(|m| m.path == std.path) {
                            bound.push(std);
                        }
                    }
                }
            }
        }
        let mut qualified_only = HashMap::new();
        for std in &imported {
            for name in std.declared() {
                if !unqualified.contains(&name) {
                    qualified_only.insert(name, *std);
                }
            }
        }
        let mut decls = Decls {
            names: HashMap::new(),
            imported,
            bound,
            qualified_only,
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
        let prelude_items = decls.prelude_items().collect::<Vec<_>>();
        for item in prelude_items {
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

    /// The prelude's items followed by those of every imported `std.*`
    /// module: everything the user module sees without declaring it.
    pub fn prelude_items(&self) -> impl Iterator<Item = &'static Item> + '_ {
        prelude::module()
            .items
            .iter()
            .chain(self.imported.iter().flat_map(|m| &m.module().items))
    }

    /// Whether the user module imports the `std.*` module `module`.
    pub fn imports(&self, module: &StdModule) -> bool {
        self.imported.iter().any(|m| m.path == module.path)
    }

    /// The imported module that declares `name` when the user module may
    /// reach it only qualified: `Db` after a plain `use std.sql`.
    pub fn qualified_only(&self, name: &str) -> Option<&'static StdModule> {
        self.qualified_only.get(name).copied()
    }

    /// The module a plain `use` binds to `name`, as `sql` for `use std.sql`.
    pub fn bound_module(&self, name: &str) -> Option<&'static StdModule> {
        self.bound.iter().copied().find(|m| m.name() == name)
    }

    /// The declared name a written path refers to, when the user module
    /// may write it: a name in scope unqualified (`Db` after
    /// `use std.sql.{Db}`), or a module's export reached through the
    /// module's name (`sql.Db`). With `see_all`, as inside the prelude and
    /// the `std` modules themselves, every declared name is in scope.
    pub fn path_name<'s>(&self, segments: &'s [Ident], see_all: bool) -> Option<&'s str> {
        match segments {
            [ident] if see_all || self.qualified_only(&ident.name).is_none() => {
                Some(ident.name.as_str())
            }
            [module, ident] => self
                .bound_module(&module.name)?
                .exports()
                .contains(&ident.name.as_str())
                .then_some(ident.name.as_str()),
            _ => None,
        }
    }

    /// The declaration of `name`, whether or not the user module can
    /// write it unqualified.
    pub fn get(&self, name: &str) -> Option<&Decl<'a>> {
        self.names.get(name)
    }

    /// The declaration of `name` when the user module can write it
    /// unqualified.
    pub fn visible(&self, name: &str) -> Option<&Decl<'a>> {
        self.qualified_only(name)
            .is_none()
            .then(|| self.names.get(name))
            .flatten()
    }

    /// All names the user module can write unqualified, for "did you
    /// mean" suggestions.
    pub fn names(&self) -> impl Iterator<Item = (&'a str, &Decl<'a>)> + '_ {
        self.names
            .iter()
            .filter(|(name, _)| self.qualified_only(name).is_none())
            .map(|(name, decl)| (*name, decl))
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
