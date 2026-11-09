//! Declaration-level rules: `cap` on types whose own fields name a
//! capability.

use oska_syntax::ast::{Item, ItemKind, Module};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::authority;
use crate::codes;
use crate::decls::{field_types, Decls};

/// Report every declaration-level error in `module`.
pub fn module(src: &str, module: &Module, decls: &Decls, diagnostics: &mut Vec<Diagnostic>) {
    for item in &module.items {
        cap_required(src, item, decls, diagnostics);
    }
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
    let Some(field) = fields
        .into_iter()
        .find(|ty| authority::bearing(ty, generics, decls))
    else {
        return;
    };
    let field_text = &src[field.span.start..field.span.end];
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
