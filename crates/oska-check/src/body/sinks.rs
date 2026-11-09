//! Sink literals: `sql"..."` and the like. A literal's tag names the
//! sink module that provides it, which must be imported, and the literal
//! has the type that module declares for it.

use oska_syntax::{Diagnostic, Edit, Span};

use super::Body;
use crate::codes;
use crate::prelude;
use crate::suggest;
use crate::ty::Ty;

impl Body<'_, '_, '_> {
    /// The sink literal `tag"..."` at `span`, which starts with the tag.
    pub(super) fn sink_literal(&mut self, tag: &str, span: Span) -> Ty {
        let tag_span = Span {
            start: span.start,
            end: span.start + tag.len(),
        };
        let Some((module, literal)) = prelude::sink_module(tag)
            .and_then(|m| m.sink_literal.as_ref().map(|literal| (m, literal)))
        else {
            let tags: Vec<&str> = prelude::STD_MODULES
                .iter()
                .filter_map(|m| m.sink_literal.as_ref().map(|s| s.tag))
                .collect();
            self.diagnostics.push(suggest::unknown(
                codes::UNKNOWN_SINK_TAG,
                format!("unknown sink literal tag `{tag}`"),
                tag,
                tag_span,
                &tags,
            ));
            return Ty::Error;
        };
        if !self.decls().imports(module) {
            let path = module.path;
            self.diagnostics.push(
                Diagnostic::new(
                    codes::SINK_NOT_IMPORTED,
                    format!("a `{tag}` literal needs `use {path}`"),
                    tag_span,
                )
                .with_fix(
                    format!("add `use {path}`"),
                    vec![Edit {
                        span: Span { start: 0, end: 0 },
                        replacement: format!("use {path}\n"),
                    }],
                ),
            );
            return Ty::Error;
        }
        Ty::named(literal.ty)
    }
}
