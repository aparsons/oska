// SPDX-License-Identifier: Apache-2.0
//! The embedded prelude: Oska source, parsed once in prelude mode.

use std::sync::OnceLock;

use oska_syntax::ast::{Item, ItemKind, Module};

/// The prelude's source text. Read `prelude.osk` for what is in scope.
pub const SOURCE: &str = include_str!("prelude.osk");

/// Primitive types, which the checker builds in rather than the prelude
/// declaring them.
pub const PRIMITIVES: &[&str] = &[
    "Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "Bool", "String",
];

/// A `std.*` module, which a module sees only once it imports it with
/// `use`. Add a module with an entry here and a `std/*.osk` file.
///
/// Most are prelude declarations, written in prelude mode like
/// `prelude.osk`, whose functions the runtime implements. A module
/// [`written in Oska`](StdModule::written_in_oska), such as `std.json`, is
/// ordinary Oska source instead: a program that imports it is checked and
/// run with the module's text as one more module of the program
/// (`link.rs`), so the checker, both backends and the differential suite
/// cover it as they cover user code.
pub struct StdModule {
    /// The path written after `use`, such as `std.sql`.
    pub path: &'static str,
    pub source: &'static str,
    /// The sink literal this module provides, if it is a sink module.
    pub sink_literal: Option<SinkLiteral>,
    /// The module is Oska source with bodies, parsed as user code, not
    /// prelude declarations.
    pub written_in_oska: bool,
}

/// A sink literal's tag and the type of a literal with that tag: with
/// `use std.sql`, `sql"..."` is an `SqlLiteral`.
pub struct SinkLiteral {
    pub tag: &'static str,
    /// A struct its module declares.
    pub ty: &'static str,
}

/// Every `std.*` module that `use` can import.
pub static STD_MODULES: &[StdModule] = &[
    StdModule {
        path: "std.sql",
        source: include_str!("std/sql.osk"),
        sink_literal: Some(SinkLiteral {
            tag: "sql",
            ty: "SqlLiteral",
        }),
        written_in_oska: false,
    },
    StdModule {
        path: "std.proc",
        source: include_str!("std/proc.osk"),
        sink_literal: None,
        written_in_oska: false,
    },
    StdModule {
        path: "std.html",
        source: include_str!("std/html.osk"),
        sink_literal: None,
        written_in_oska: false,
    },
    StdModule {
        path: "std.sha2",
        source: include_str!("std/sha2.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.hmac",
        source: include_str!("std/hmac.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.ed25519",
        source: include_str!("std/ed25519.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.sshsig",
        source: include_str!("std/sshsig.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.json",
        source: include_str!("std/json.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.mcp",
        source: include_str!("std/mcp.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
];

/// The `std` modules written in Oska that `module` imports, in the order
/// of their first `use`.
pub fn oska_std_imports(module: &Module) -> Vec<&'static StdModule> {
    let mut found: Vec<&'static StdModule> = Vec::new();
    for item in &module.items {
        let ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let path: Vec<&str> = u.path.iter().map(|s| s.name.as_str()).collect();
        let Some(std) = std_module(&path.join(".")) else {
            continue;
        };
        if std.written_in_oska && !found.iter().any(|m| m.path == std.path) {
            found.push(std);
        }
    }
    found
}

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

/// The sink literal whose type is named `ty`, such as `sql` for
/// `SqlLiteral`.
pub fn sink_literal_typed(ty: &str) -> Option<&'static SinkLiteral> {
    STD_MODULES
        .iter()
        .filter_map(|m| m.sink_literal.as_ref())
        .find(|s| s.ty == ty)
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

    /// The names of the module's `pub` functions, types, and traits: what
    /// a `use` brace group may name.
    pub fn exports(&self) -> Vec<&'static str> {
        self.module()
            .items
            .iter()
            .filter(|item| item.is_public())
            .filter_map(item_name)
            .collect()
    }

    /// Whether the module exports `name`.
    pub fn exports_name(&self, name: &str) -> bool {
        self.exports().contains(&name)
    }

    /// Every function, type, and trait name the module declares, `pub` or
    /// not.
    pub fn declared(&self) -> Vec<&'static str> {
        self.module().items.iter().filter_map(item_name).collect()
    }

    /// The name a plain `use` binds: the path's last segment, as `sql`
    /// for `std.sql`.
    pub fn name(&self) -> &'static str {
        self.path.rsplit('.').next().unwrap_or(self.path)
    }
}

/// The function, type, or trait name `item` declares.
pub fn item_name(item: &Item) -> Option<&str> {
    match &item.kind {
        ItemKind::Fn(f) => Some(f.name.name.as_str()),
        ItemKind::Struct(s) => Some(s.name.name.as_str()),
        ItemKind::Enum(e) => Some(e.name.name.as_str()),
        ItemKind::Trait(t) => Some(t.name.name.as_str()),
        _ => None,
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
        resolve::prelude_module(super::module(), &decls, &mut diagnostics);
        let found: Vec<String> = diagnostics
            .iter()
            .map(|d| format!("{d}: {:?}", &super::SOURCE[d.span.start..d.span.end]))
            .collect();
        assert!(found.is_empty(), "prelude.osk: {found:#?}");
    }

    #[test]
    fn every_name_in_each_std_module_resolves_once_imported() {
        for std in super::STD_MODULES {
            // A module written in Oska sees the `std` modules it imports,
            // as a program that imports it does; it is also checked whole,
            // linked with them, as that program is.
            let mut uses = vec![std.path];
            if std.written_in_oska {
                let report = crate::check_module(std.source);
                assert!(
                    report.diagnostics.is_empty(),
                    "{}: {:#?}",
                    std.path,
                    report.diagnostics
                );
                let ast = oska_syntax::parse_module(std.source).expect("the module parses");
                uses.extend(super::oska_std_imports(&ast).iter().map(|m| m.path));
            }
            let importer: String = uses.iter().map(|path| format!("use {path}\n")).collect();
            let importer = oska_syntax::parse_module(&importer).expect("a use parses");
            let mut diagnostics = Vec::new();
            let decls = Decls::collect(&importer, &mut diagnostics);
            resolve::prelude_module(std.module(), &decls, &mut diagnostics);
            let found: Vec<String> = diagnostics
                .iter()
                .map(|d| format!("{d}: {:?}", &std.source[d.span.start..d.span.end]))
                .collect();
            assert!(found.is_empty(), "{}: {found:#?}", std.path);
        }
    }

    #[test]
    fn each_sink_literal_type_is_exported_by_its_module() {
        for std in super::STD_MODULES {
            if let Some(literal) = &std.sink_literal {
                assert!(
                    std.exports_name(literal.ty),
                    "{} does not export `{}`",
                    std.path,
                    literal.ty
                );
            }
        }
    }
}
