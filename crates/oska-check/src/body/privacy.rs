//! Privacy below the item, across a package's modules: a struct's field
//! without `pub`, an inherent method without `pub(pkg)` or `pub`, and a
//! private trait's methods are for the module that declares them.
//! Linking (`link.rs`) checks every name a module writes; these are
//! reached through a value's type instead, so the body checks them where
//! it resolves a field, a method or a constructor. Each is reported as a
//! private item is (`E0206`), with a fix in the declaring file.
//!
//! A single file is one module, so nothing here applies to it.

use oska_syntax::ast::Visibility;
use oska_syntax::{Diagnostic, Edit, Span};

use super::Body;
use crate::codes;
use crate::items::{Declared, FnSig};
use crate::link::{self, Modules};

impl Body<'_, '_, '_> {
    /// The module of the code being checked.
    fn here(&self) -> &str {
        &self.out.module
    }

    /// The module that declares what starts at `offset`, when it is not
    /// the module being checked.
    fn foreign(&self, offset: usize) -> Option<String> {
        let module = self.out.modules.at(offset);
        (module != self.here()).then(|| module.to_string())
    }

    /// Where a visibility marker goes in `declared`: after its attributes.
    fn marker_at(&self, declared: &Declared) -> usize {
        match declared.attributes_end {
            Some(end) => {
                let rest = self.src.get(end..).unwrap_or_default();
                end + (rest.len() - rest.trim_start().len())
            }
            None => declared.start,
        }
    }

    /// Report the use at `span` of `what`, private to `module`, with a fix
    /// that inserts `marker` at `at`.
    fn private(&mut self, what: &str, module: &str, marker: &str, at: usize, span: Span) {
        let shown = marker.trim_end();
        let diagnostic = Diagnostic::new(
            codes::PRIVATE_ITEM,
            format!(
                "{what} is private to `{}.{module}`; mark it `{shown}` to use it from other \
                 modules of the package",
                link::PKG
            ),
            span,
        )
        .with_fix(format!("mark it `{shown}`"), vec![Edit::insert(at, marker)]);
        self.out.diagnostics.push(diagnostic);
    }

    /// Whether `sig`, a method of `type_name` called at `span`, may be
    /// called here: an inherent method must be `pub(pkg)` or `pub`, or in
    /// this module; a trait's method needs the trait to be. Reports one
    /// that may not.
    pub(super) fn method_visible(&mut self, sig: &FnSig, type_name: &str, span: Span) -> bool {
        let (declared, what) = match &sig.trait_name {
            None => (
                sig.declared,
                format!(
                    "the method `{}` of `{}`",
                    sig.name,
                    Modules::display(type_name, self.here())
                ),
            ),
            Some(trait_name) => (
                self.items.traits.get(trait_name).and_then(|t| t.declared),
                format!(
                    "the trait `{}` of the method `{}`",
                    link::unqualified(trait_name),
                    sig.name
                ),
            ),
        };
        let Some(declared) = declared else {
            return true;
        };
        if declared.visibility != Visibility::Private {
            return true;
        }
        let Some(module) = self.foreign(declared.start) else {
            return true;
        };
        let at = self.marker_at(&declared);
        self.private(&what, &module, "pub(pkg) ", at, span);
        false
    }

    /// Whether field `index` of the struct `type_name` may be used here:
    /// it is `pub`, or the struct is this module's. Reports one that may
    /// not, as the field `name` at `span`.
    pub(super) fn field_visible(
        &mut self,
        type_name: &str,
        index: usize,
        name: &str,
        span: Span,
    ) -> bool {
        let Some((declared, private)) = self
            .items
            .structs
            .get(type_name)
            .and_then(|s| s.declared.clone())
        else {
            return true;
        };
        let Some(&(_, at)) = private.iter().find(|(i, _)| *i == index) else {
            return true;
        };
        let Some(module) = self.foreign(declared.start) else {
            return true;
        };
        let what = format!(
            "the field `{name}` of `{}`",
            Modules::display(type_name, self.here())
        );
        self.private(&what, &module, "pub ", at, span);
        false
    }

    /// Whether the struct `type_name` may be built or taken apart here,
    /// at `span`: every field is `pub`, or the struct is this module's.
    /// Reports one that may not, with a fix that marks each private field
    /// `pub`.
    pub(super) fn fields_visible(&mut self, type_name: &str, span: Span) -> bool {
        let Some((declared, private)) = self
            .items
            .structs
            .get(type_name)
            .and_then(|s| s.declared.clone())
        else {
            return true;
        };
        if private.is_empty() {
            return true;
        }
        let Some(module) = self.foreign(declared.start) else {
            return true;
        };
        let diagnostic = Diagnostic::new(
            codes::PRIVATE_ITEM,
            format!(
                "`{}` has fields private to `{}.{module}`, so only that module can build it or \
                 match its fields; mark them `pub` to do so from other modules of the package",
                Modules::display(type_name, self.here()),
                link::PKG
            ),
            span,
        )
        .with_fix(
            "mark its fields `pub`",
            private
                .iter()
                .map(|&(_, at)| Edit::insert(at, "pub "))
                .collect(),
        );
        self.out.diagnostics.push(diagnostic);
        false
    }
}
