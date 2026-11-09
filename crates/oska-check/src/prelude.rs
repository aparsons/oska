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

/// A `std.*` module: prelude declarations, written in prelude mode like
/// `prelude.osk`, that a module sees only once it imports them with `use`.
/// Add a module with an entry here and a `std/*.osk` file; add items to
/// one by appending declarations to its file.
pub struct StdModule {
    /// The path written after `use`, such as `std.sql`.
    pub path: &'static str,
    pub source: &'static str,
    /// The sink literal this module provides, if it is a sink module.
    pub sink_literal: Option<SinkLiteral>,
}

/// A sink literal's tag and the type of a literal with that tag: with
/// `use std.sql`, `sql"..."` is an `SqlLiteral`.
pub struct SinkLiteral {
    pub tag: &'static str,
    /// A struct its module declares.
    pub ty: &'static str,
}

/// Every `std.*` module that `use` can import.
pub static STD_MODULES: &[StdModule] = &[StdModule {
    path: "std.sql",
    source: include_str!("std/sql.osk"),
    sink_literal: Some(SinkLiteral {
        tag: "sql",
        ty: "SqlLiteral",
    }),
}];

/// The `std.*` module at `path`.
pub fn std_module(path: &str) -> Option<&'static StdModule> {
    STD_MODULES.iter().find(|m| m.path == path)
}

/// The module whose sink literal has `tag`.
pub fn sink_module(tag: &str) -> Option<&'static StdModule> {
    STD_MODULES
        .iter()
        .find(|m| m.sink_literal.as_ref().is_some_and(|s| s.tag == tag))
}

impl StdModule {
    /// The module's declarations, parsed once.
    pub fn module(&self) -> &'static Module {
        static MODULES: OnceLock<Vec<Module>> = OnceLock::new();
        let modules = MODULES.get_or_init(|| {
            STD_MODULES
                .iter()
                .map(|m| match oska_syntax::parse_prelude(m.source) {
                    Ok(module) => module,
                    Err(err) => panic!("the embedded `{}` should parse: {err}", m.path),
                })
                .collect()
        });
        let index = STD_MODULES
            .iter()
            .position(|m| m.path == self.path)
            .expect("every std module is in STD_MODULES");
        &modules[index]
    }
}

/// Prelude functions that `--release` rejects.
pub const DBG: &str = "dbg";
pub const TODO: &str = "todo";

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
