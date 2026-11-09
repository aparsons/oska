//! Name resolution for the type and trait names in signatures and
//! declarations. Names in function bodies are resolved by `body.rs`.

use oska_syntax::ast::{FnDecl, GenericParam, Ident, ItemKind, Module, Type, TypeKind};
use oska_syntax::{Diagnostic, Span};

use crate::codes;
use crate::collect::{DeclKind, Decls};
use crate::prelude;
use crate::suggest;

/// Report every unresolved or misused type and trait name in `module`.
pub fn module(module: &Module, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    let mut resolver = Resolver {
        decls,
        diagnostics,
        generics: Vec::new(),
        self_in_scope: false,
    };
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => resolver.function(f),
            ItemKind::Const(c) => resolver.ty(&c.ty),
            ItemKind::Struct(s) => {
                resolver.enter(&s.generics, false);
                for ty in crate::collect::field_types(&s.fields) {
                    resolver.ty(ty);
                }
                resolver.exit(&s.generics);
            }
            ItemKind::Enum(e) => {
                resolver.enter(&e.generics, false);
                for variant in &e.variants {
                    for ty in crate::collect::field_types(&variant.fields) {
                        resolver.ty(ty);
                    }
                }
                resolver.exit(&e.generics);
            }
            ItemKind::Trait(t) => {
                resolver.enter(&t.generics, true);
                resolver.methods(&t.methods);
                resolver.exit(&t.generics);
            }
            ItemKind::Impl(imp) => {
                resolver.enter(&imp.generics, false);
                if let Some(trait_ref) = &imp.trait_ref {
                    resolver.trait_ref(trait_ref);
                }
                resolver.ty(&imp.target);
                resolver.self_in_scope = true;
                resolver.methods(&imp.methods);
                resolver.exit(&imp.generics);
            }
            ItemKind::Use(u) => resolver.use_decl(&u.path),
            ItemKind::Test(_) => {}
        }
    }
}

struct Resolver<'d, 'a> {
    decls: &'d Decls<'a>,
    diagnostics: &'d mut Vec<Diagnostic>,
    /// Generic parameters in scope, innermost last.
    generics: Vec<String>,
    /// Inside a `trait` or `impl`, where `Self` names the type.
    self_in_scope: bool,
}

/// Which namespace a name is expected in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Type,
    Trait,
}

impl Resolver<'_, '_> {
    /// Bring `generics` into scope and resolve their bounds.
    fn enter(&mut self, generics: &[GenericParam], self_in_scope: bool) {
        self.generics
            .extend(generics.iter().map(|g| g.name.name.clone()));
        self.self_in_scope = self_in_scope;
        for g in generics {
            for bound in &g.bounds {
                self.trait_ref(bound);
            }
        }
    }

    fn exit(&mut self, generics: &[GenericParam]) {
        self.generics.truncate(self.generics.len() - generics.len());
        self.self_in_scope = false;
    }

    fn methods(&mut self, methods: &[oska_syntax::ast::Item]) {
        for method in methods {
            if let ItemKind::Fn(f) = &method.kind {
                self.function(f);
            }
        }
    }

    fn function(&mut self, f: &FnDecl) {
        let self_in_scope = self.self_in_scope;
        self.enter(&f.generics, self_in_scope);
        for param in &f.params {
            self.ty(&param.ty);
        }
        if let Some(ret) = &f.return_type {
            self.ty(ret);
        }
        self.exit(&f.generics);
        self.self_in_scope = self_in_scope;
    }

    /// Resolve a type written where a type is expected.
    fn ty(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Unit => {}
            TypeKind::Fn {
                params,
                return_type,
                ..
            } => {
                for param in params {
                    self.ty(param);
                }
                if let Some(ret) = return_type {
                    self.ty(ret);
                }
            }
            TypeKind::Path { segments, args } => {
                self.path(segments, Expect::Type);
                for arg in args {
                    self.ty(arg);
                }
            }
        }
    }

    /// Resolve a type written where a trait is expected: a bound, or the
    /// trait of an `impl Trait for Type`.
    fn trait_ref(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Path { segments, args } => {
                self.path(segments, Expect::Trait);
                for arg in args {
                    self.ty(arg);
                }
            }
            // The parser only produces paths in trait position.
            TypeKind::Unit | TypeKind::Fn { .. } => {}
        }
    }

    fn path(&mut self, segments: &[Ident], expect: Expect) {
        let [ident] = segments else {
            // Only `std.*` modules can be imported, and none exist yet.
            let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
                return;
            };
            let span = Span {
                start: first.span.start,
                end: last.span.end,
            };
            let names: Vec<&str> = segments.iter().map(|s| s.name.as_str()).collect();
            self.diagnostics.push(Diagnostic::new(
                codes::UNKNOWN_NAME,
                format!("unknown {} `{}`", expect.noun(), names.join(".")),
                span,
            ));
            return;
        };
        let name = ident.name.as_str();
        let found = if self.is_type_variable(name) {
            Some(Expect::Type)
        } else {
            self.decls.get(name).map(|d| match d.kind {
                DeclKind::Trait => Expect::Trait,
                _ => Expect::Type,
            })
        };
        match found {
            Some(found) if found == expect => {}
            Some(found) => self.diagnostics.push(Diagnostic::new(
                codes::WRONG_NAMESPACE,
                format!("`{name}` is a {}, not a {}", found.noun(), expect.noun()),
                ident.span,
            )),
            None => self.unknown(ident, expect),
        }
    }

    fn is_type_variable(&self, name: &str) -> bool {
        self.generics.iter().any(|g| g == name) || (self.self_in_scope && name == "Self")
    }

    fn unknown(&mut self, ident: &Ident, expect: Expect) {
        let mut candidates: Vec<&str> = self
            .decls
            .names()
            .filter(|(_, decl)| (decl.kind == DeclKind::Trait) == (expect == Expect::Trait))
            .map(|(name, _)| name)
            .collect();
        if expect == Expect::Type {
            candidates.extend(self.generics.iter().map(String::as_str));
            if self.self_in_scope {
                candidates.push("Self");
            }
        }
        let name = &ident.name;
        let message = format!("unknown {} `{name}`", expect.noun());
        self.diagnostics.push(suggest::unknown(
            codes::UNKNOWN_NAME,
            message,
            name,
            ident.span,
            &candidates,
        ));
    }

    /// `use std.module`: only the standard library can be imported, and
    /// only modules that exist. The brace group is not checked: its names
    /// are only known once the module is.
    fn use_decl(&mut self, path: &[Ident]) {
        let (Some(first), Some(last)) = (path.first(), path.last()) else {
            return;
        };
        let span = Span {
            start: first.span.start,
            end: last.span.end,
        };
        let written: Vec<&str> = path.iter().map(|s| s.name.as_str()).collect();
        let written = written.join(".");
        if first.name != "std" {
            self.diagnostics.push(Diagnostic::new(
                codes::USE_OUTSIDE_STD,
                format!(
                    "`use {written}` imports from outside the standard library; \
                     only `std.*` modules can be imported"
                ),
                span,
            ));
            return;
        }
        if !prelude::STD_MODULES.contains(&written.as_str()) {
            self.diagnostics.push(suggest::unknown(
                codes::UNKNOWN_MODULE,
                format!("unknown module `{written}`"),
                &written,
                span,
                prelude::STD_MODULES,
            ));
        }
    }
}

impl Expect {
    fn noun(self) -> &'static str {
        match self {
            Expect::Type => "type",
            Expect::Trait => "trait",
        }
    }
}
