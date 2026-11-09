//! The embedded prelude: Oska source, parsed once in prelude mode.

use std::sync::OnceLock;

use oska_syntax::ast::Module;

/// The prelude's source text. Read `prelude.osk` for what is in scope.
pub const SOURCE: &str = include_str!("prelude.osk");

/// Primitive types, which the checker builds in rather than the prelude
/// declaring them.
pub const PRIMITIVES: &[&str] = &[
    "Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "Bool", "String",
];

/// The `std.*` modules that `use` can import. There are none yet, so
/// every `use` is an error for now.
pub const STD_MODULES: &[&str] = &[];

/// The parsed prelude.
pub fn module() -> &'static Module {
    static MODULE: OnceLock<Module> = OnceLock::new();
    MODULE.get_or_init(|| match oska_syntax::parse_prelude(SOURCE) {
        Ok(module) => module,
        Err(err) => panic!("the embedded prelude should parse: {err}"),
    })
}

#[cfg(test)]
mod tests {
    use crate::{collect::Decls, resolve};

    #[test]
    fn every_name_in_the_prelude_resolves() {
        let empty = oska_syntax::parse_module("").expect("empty module");
        let mut diagnostics = Vec::new();
        let decls = Decls::collect(&empty, &mut diagnostics);
        resolve::module(super::module(), &decls, &mut diagnostics);
        let found: Vec<String> = diagnostics
            .iter()
            .map(|d| format!("{d}: {:?}", &super::SOURCE[d.span.start..d.span.end]))
            .collect();
        assert!(found.is_empty(), "prelude.osk: {found:#?}");
    }
}
