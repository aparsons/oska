//! Name resolution for the type and trait names in signatures and
//! declarations. Function bodies are not resolved yet.

use oska_syntax::ast::{FnDecl, GenericParam, Ident, ItemKind, Module, Type, TypeKind};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::codes;
use crate::decls::{DeclKind, Decls};

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
                for ty in crate::decls::field_types(&s.fields) {
                    resolver.ty(ty);
                }
                resolver.exit(&s.generics);
            }
            ItemKind::Enum(e) => {
                resolver.enter(&e.generics, false);
                for variant in &e.variants {
                    for ty in crate::decls::field_types(&variant.fields) {
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
            ItemKind::Use(_) | ItemKind::Test(_) => {}
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
        let noun = expect.noun();
        let diagnostic = match closest(name, &candidates) {
            Some(suggestion) => Diagnostic::new(
                codes::UNKNOWN_NAME,
                format!("unknown {noun} `{name}`; did you mean `{suggestion}`?"),
                ident.span,
            )
            .with_fix(
                format!("replace with `{suggestion}`"),
                vec![Edit {
                    span: ident.span,
                    replacement: suggestion.to_string(),
                }],
            ),
            None => Diagnostic::new(
                codes::UNKNOWN_NAME,
                format!("unknown {noun} `{name}`"),
                ident.span,
            ),
        };
        self.diagnostics.push(diagnostic);
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

/// The single candidate closest to `name`, if one is close enough: at
/// most a third of the name's length in edits (at least one), ignoring
/// case. A tie means no suggestion.
fn closest<'c>(name: &str, candidates: &[&'c str]) -> Option<&'c str> {
    let limit = (name.chars().count() / 3).max(1);
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for &candidate in candidates {
        let d = distance(&name.to_lowercase(), &candidate.to_lowercase());
        if d > limit {
            continue;
        }
        match best {
            Some((best_d, best_name)) if d == best_d && best_name != candidate => tied = true,
            Some((best_d, _)) if d >= best_d => {}
            _ => {
                best = Some((d, candidate));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, candidate)| candidate)
    }
}

/// Edit distance, counting an adjacent transposition as one edit.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}
