//! Declaration-level rules: `cap` on types whose own fields name a
//! capability, `main` as the root of authority, and capitalization
//! (ADR 0005).

use oska_syntax::ast::{
    Fields, FnDecl, GenericParam, Ident, Item, ItemKind, Module, Type, TypeKind,
};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::authority::capability_bearing;
use crate::codes;
use crate::collect::{field_types, DeclKind, Decls};
use crate::items::Lower;
use crate::ty::{self, SCOPE};

/// Report every declaration-level error in `module`.
pub fn module(src: &str, module: &Module, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    for item in &module.items {
        cap_required(src, item, decls, diagnostics);
        capitalization(item, diagnostics);
        scope_confined(item, diagnostics);
        if let ItemKind::Fn(f) = &item.kind {
            if f.name.name == "main" {
                main(src, module, f, decls, diagnostics);
                main_contract(f, diagnostics);
            }
        }
        if let ItemKind::Impl(imp) = &item.kind {
            if imp.trait_ref.is_some() {
                impl_contracts(&imp.methods, diagnostics);
            }
        }
    }
}

/// The span of every `requires` and `ensures` clause `f` declares, in
/// source order.
fn clause_spans(f: &FnDecl) -> Vec<Span> {
    let mut spans: Vec<Span> = f
        .requires
        .iter()
        .map(|c| c.span)
        .chain(f.ensures.iter().map(|c| c.span))
        .collect();
    spans.sort_by_key(|span| span.start);
    spans
}

/// A method of `impl Trait for Type` is held to the contract its trait
/// method declares, and has none of its own: the trait is the one place a
/// method's contract lives (`E0218`).
fn impl_contracts(methods: &[Item], diagnostics: &mut Vec<Diagnostic>) {
    for method in methods {
        let ItemKind::Fn(f) = &method.kind else {
            continue;
        };
        for span in clause_spans(f) {
            diagnostics.push(Diagnostic::new(
                codes::IMPL_CONTRACT,
                format!(
                    "`{}` implements a trait method, so its contract belongs on the trait's \
                     declaration of it; move the clause there",
                    f.name.name
                ),
                span,
            ));
        }
    }
}

/// Nothing calls `main`, so a contract on it has no caller to blame
/// (`E0219`).
fn main_contract(f: &FnDecl, diagnostics: &mut Vec<Diagnostic>) {
    for span in clause_spans(f) {
        diagnostics.push(Diagnostic::new(
            codes::MAIN_CONTRACT,
            "`main` cannot have a contract: nothing calls it, so there is no caller to blame; \
             check the condition in its body instead",
            span,
        ));
    }
}

/// The `scope` handle lives only as long as its block, so `Scope` cannot
/// appear in a return type or a field type, even inside a wrapper such as
/// `Option<Scope>`. It stays allowed as a parameter type, including a
/// function type's parameter.
fn scope_confined(item: &Item, diagnostics: &mut Vec<Diagnostic>) {
    const RETURNED: &str = "be returned from a function";
    const STORED: &str = "be stored in a field";
    let positions: Vec<(&Type, &str)> = match &item.kind {
        ItemKind::Fn(f) => f.return_type.iter().map(|t| (t, RETURNED)).collect(),
        ItemKind::Impl(imp) => returns(&imp.methods)
            .into_iter()
            .map(|t| (t, RETURNED))
            .collect(),
        ItemKind::Trait(t) => returns(&t.methods)
            .into_iter()
            .map(|t| (t, RETURNED))
            .collect(),
        ItemKind::Struct(s) => field_types(&s.fields)
            .into_iter()
            .map(|t| (t, STORED))
            .collect(),
        ItemKind::Enum(e) => e
            .variants
            .iter()
            .flat_map(|v| field_types(&v.fields))
            .map(|t| (t, STORED))
            .collect(),
        ItemKind::Use(_) | ItemKind::Const(_) | ItemKind::Test(_) => Vec::new(),
    };
    for (ty, place) in positions {
        if holds_scope(ty) {
            diagnostics.push(scope_escapes(ty.span, place));
        }
    }
}

/// The written return types of `methods`.
fn returns(methods: &[Item]) -> Vec<&Type> {
    methods
        .iter()
        .filter_map(|m| match &m.kind {
            ItemKind::Fn(f) => f.return_type.as_ref(),
            _ => None,
        })
        .collect()
}

/// `E0292`: a `Scope` that would outlive its block.
pub fn scope_escapes(span: Span, place: &str) -> Diagnostic {
    Diagnostic::new(
        codes::SCOPE_ESCAPES,
        format!(
            "a `Scope` handle cannot outlive its `scope` block, so it cannot {place}; \
             pass it as a parameter instead"
        ),
        span,
    )
}

/// `ty` names the prelude's `Scope` anywhere outside a function type's
/// parameters.
fn holds_scope(ty: &Type) -> bool {
    match &ty.kind {
        TypeKind::Unit => false,
        TypeKind::Fn { return_type, .. } => return_type.as_deref().is_some_and(holds_scope),
        TypeKind::Path { segments, args } => {
            matches!(segments.as_slice(), [only] if only.name == SCOPE)
                || args.iter().any(holds_scope)
        }
    }
}

/// Types, variants, traits, and type parameters start uppercase.
/// Functions, fields, and parameters start lowercase.
fn capitalization(item: &Item, diagnostics: &mut Vec<Diagnostic>) {
    let mut names = Names::default();
    match &item.kind {
        ItemKind::Fn(f) => names.function(f),
        ItemKind::Struct(s) => {
            names.upper(&s.name, "type");
            names.generics(&s.generics);
            names.fields(&s.fields);
        }
        ItemKind::Enum(e) => {
            names.upper(&e.name, "type");
            names.generics(&e.generics);
            for variant in &e.variants {
                names.upper(&variant.name, "variant");
                names.fields(&variant.fields);
            }
        }
        ItemKind::Trait(t) => {
            names.upper(&t.name, "trait");
            names.generics(&t.generics);
            names.methods(&t.methods);
        }
        ItemKind::Impl(imp) => {
            names.generics(&imp.generics);
            names.methods(&imp.methods);
        }
        ItemKind::Use(_) | ItemKind::Const(_) | ItemKind::Test(_) => {}
    }
    diagnostics.extend(names.diagnostics);
}

/// Collects capitalization errors for the names one item declares.
#[derive(Default)]
struct Names {
    diagnostics: Vec<Diagnostic>,
}

impl Names {
    /// A name another module's item is declared under, such as
    /// `pkg.util.Item` (`link.rs`), is judged by its own last segment.
    fn upper(&mut self, ident: &Ident, noun: &str) {
        let name = crate::link::unqualified(&ident.name);
        if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
            self.diagnostics.push(Diagnostic::new(
                codes::NOT_UPPERCASE,
                format!("{noun} name `{name}` must start with an uppercase letter"),
                ident.span,
            ));
        }
    }

    /// A leading `_` counts as lowercase.
    fn lower(&mut self, ident: &Ident, noun: &str) {
        let name = crate::link::unqualified(&ident.name);
        if name.starts_with(|c: char| c.is_ascii_uppercase()) {
            self.diagnostics.push(Diagnostic::new(
                codes::NOT_LOWERCASE,
                format!("{noun} name `{name}` must start with a lowercase letter"),
                ident.span,
            ));
        }
    }

    fn generics(&mut self, generics: &[GenericParam]) {
        for g in generics {
            self.upper(&g.name, "type parameter");
        }
    }

    fn fields(&mut self, fields: &Fields) {
        if let Fields::Named(fields) = fields {
            for field in fields {
                if let Some(name) = &field.name {
                    self.lower(name, "field");
                }
            }
        }
    }

    fn methods(&mut self, methods: &[Item]) {
        for method in methods {
            if let ItemKind::Fn(f) = &method.kind {
                self.function(f);
            }
        }
    }

    fn function(&mut self, f: &FnDecl) {
        self.lower(&f.name, "function");
        self.generics(&f.generics);
        for param in &f.params {
            self.lower(&param.name, "parameter");
        }
    }
}

/// The directory and file capabilities. The runtime has no root to give
/// one, so `main` cannot declare it (ADR 0008).
const CONFINED: [&str; 5] = ["Dir", "DirRead", "DirWrite", "FileRead", "FileWrite"];

/// `main` is the root of authority: it takes only built-in capabilities
/// and `Args`, and returns `()` or `Result<(), E>` with `E: Display`.
fn main(src: &str, module: &Module, f: &FnDecl, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    for param in &f.params {
        // An unknown name is already reported by name resolution.
        let allowed = match single_name(&param.ty) {
            Some(name) => match decls.get(name) {
                Some(decl) => name == ty::ARGS || decl.kind == DeclKind::Capability,
                None => true,
            },
            None => false,
        };
        let confined = single_name(&param.ty).filter(|name| {
            CONFINED.contains(name) && decls.get(name).is_some_and(|d| d.in_prelude)
        });
        if let Some(name) = confined {
            diagnostics.push(Diagnostic::new(
                codes::MAIN_CONFINED,
                format!(
                    "`main` cannot take `{name}`: only attenuation makes a directory or file \
                     capability, with `restrict_to` or `restrict_to_file` on `Fs`, `FsRead` or \
                     `FsWrite`, so declare one of those and narrow it"
                ),
                param.ty.span,
            ));
        } else if !allowed {
            diagnostics.push(Diagnostic::new(
                codes::MAIN_PARAM,
                format!(
                    "`main` can only take built-in capabilities and `Args`, not `{}`",
                    text(src, &param.ty)
                ),
                param.ty.span,
            ));
        }
    }
    let Some(ret) = &f.return_type else {
        return;
    };
    if let TypeKind::Unit = ret.kind {
        return;
    }
    let error = match &ret.kind {
        TypeKind::Path { segments, args } if is_named(segments, ty::RESULT) => {
            match args.as_slice() {
                [ok, error] if matches!(ok.kind, TypeKind::Unit) => Some(error),
                _ => None,
            }
        }
        _ => None,
    };
    let Some(error) = error else {
        diagnostics.push(Diagnostic::new(
            codes::MAIN_RETURN,
            format!(
                "`main` must return `()` or `Result<(), E>`, not `{}`",
                text(src, ret)
            ),
            ret.span,
        ));
        return;
    };
    let display = single_name(error).is_some_and(|name| {
        decls
            .prelude_items()
            .any(|item| implements_display(item, name))
            || module
                .items
                .iter()
                .any(|item| implements_display(item, name))
    });
    // An unknown name is already reported by name resolution.
    let known = single_name(error).is_some_and(|name| decls.get(name).is_some());
    if known && !display {
        diagnostics.push(Diagnostic::new(
            codes::MAIN_RETURN,
            format!(
                "`main` can return `Result<(), {0}>` only if `{0}` implements `Display`",
                text(src, error)
            ),
            error.span,
        ));
    }
}

/// Whether `item` is an `impl Display for name`.
fn implements_display(item: &Item, name: &str) -> bool {
    match &item.kind {
        ItemKind::Impl(imp) => {
            imp.trait_ref
                .as_ref()
                .is_some_and(|t| single_name(t) == Some(ty::DISPLAY))
                && single_name(&imp.target) == Some(name)
        }
        _ => false,
    }
}

/// The name of a one-segment path type with no type arguments.
fn single_name(ty: &Type) -> Option<&str> {
    match &ty.kind {
        TypeKind::Path { segments, args } if args.is_empty() => match segments.as_slice() {
            [ident] => Some(ident.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn is_named(segments: &[oska_syntax::ast::Ident], name: &str) -> bool {
    matches!(segments, [ident] if ident.name == name)
}

fn text<'s>(src: &'s str, ty: &Type) -> &'s str {
    &src[ty.span.start..ty.span.end]
}

/// A struct or enum whose own fields name a capability must be `cap`.
fn cap_required(src: &str, item: &Item, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    let (keyword, name, generics, fields) = match &item.kind {
        ItemKind::Struct(s) if !s.cap => ("struct", &s.name, &s.generics, field_types(&s.fields)),
        ItemKind::Enum(e) if !e.cap => {
            let fields = e.variants.iter().flat_map(|v| field_types(&v.fields));
            ("enum", &e.name, &e.generics, fields.collect())
        }
        _ => return,
    };
    // A field typed by a type parameter does not count: the type is
    // capability-bearing only once instantiated.
    let lower = Lower::new(decls).with_generics(generics);
    let Some(field) = fields
        .into_iter()
        .find(|ty| capability_bearing(&lower.ty(ty), decls))
    else {
        return;
    };
    let field_text = text(src, field);
    let mut diagnostic = Diagnostic::new(
        codes::CAP_REQUIRED,
        format!(
            "`{}` holds the capability-bearing type `{field_text}`, so it must be declared `cap {keyword}`",
            name.name
        ),
        name.span,
    );
    // Between the item's start and its name, the keyword is the last word.
    if let Some(offset) = src[item.span.start..name.span.start].rfind(keyword) {
        let at = item.span.start + offset;
        diagnostic = diagnostic.with_fix(
            format!("declare it `cap {keyword}`"),
            vec![Edit {
                span: Span { start: at, end: at },
                replacement: "cap ".to_string(),
            }],
        );
    }
    diagnostics.push(diagnostic);
}
