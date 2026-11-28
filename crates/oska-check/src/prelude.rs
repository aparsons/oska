// SPDX-License-Identifier: Apache-2.0
//! The embedded prelude: Oska source, parsed once in prelude mode.

use std::sync::OnceLock;

use oska_syntax::ast::{Item, ItemKind, Module};

/// The prelude's source text. Read `prelude.osk` for what is in scope.
pub const SOURCE: &str = include_str!("prelude.osk");

/// Primitive types, which the checker builds in rather than the prelude
/// declaring them.
pub const PRIMITIVES: &[&str] = &[
    "Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "Float", "Bool", "String",
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
        path: "std.net_error",
        source: include_str!("std/net_error.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.dns",
        source: include_str!("std/dns.osk"),
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
        path: "std.montgomery",
        source: include_str!("std/montgomery.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.ecdsa",
        source: include_str!("std/ecdsa.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.p256",
        source: include_str!("std/p256.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.rsa",
        source: include_str!("std/rsa.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.x509",
        source: include_str!("std/x509.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.x509_roots",
        source: include_str!("std/x509_roots.osk"),
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
        path: "std.x25519",
        source: include_str!("std/x25519.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.hkdf",
        source: include_str!("std/hkdf.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.chacha20poly1305",
        source: include_str!("std/chacha20poly1305.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.aes128gcm",
        source: include_str!("std/aes128gcm.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.tls",
        source: include_str!("std/tls.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.http",
        source: include_str!("std/http.osk"),
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
        path: "std.map",
        source: include_str!("std/map.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.store",
        source: include_str!("std/store.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.core",
        source: include_str!("std/core.osk"),
        sink_literal: None,
        written_in_oska: true,
    },
    StdModule {
        path: "std.math",
        source: include_str!("std/math.osk"),
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

/// The `std` modules written in Oska whose `pub` names are prelude names:
/// a program uses them with no `use`, as it uses `List`. The module is
/// still `std.*` source, written in Oska and run by both backends, so it
/// is joined to a program only when the program's text writes one of the
/// names (a program that never says `Map` or `Entry` is unchanged).
/// `use std.map` still works.
pub const PRELUDE_STD: &[(&str, &[&str])] = &[("std.map", &["Map", "Entry"])];

/// The methods whose bodies are Oska functions of `std.core` (ADR 0034),
/// by type: `List` (functions `list_<name>`), `Option` (`option_<name>`)
/// and `Result` (`result_<name>`). The prelude declares each body-less; a
/// call of one resolves to that function of `std.core`, so a program whose
/// text calls one is joined to the module.
pub const CORE_METHODS: &[(&str, &[&str])] = &[
    (
        "List",
        &[
            "filter",
            "fold",
            "any",
            "all",
            "find",
            "contains",
            "first",
            "last",
            "is_empty",
            "reverse",
            "concat",
            "sorted",
            "sorted_by",
            "sort",
        ],
    ),
    (
        "Option",
        &["map", "and_then", "unwrap_or_else", "is_none", "filter"],
    ),
    (
        "Result",
        &[
            "map",
            "map_err",
            "and_then",
            "unwrap_or_else",
            "is_err",
            "ok",
            "err",
        ],
    ),
];

/// The `std.core` function that the prelude method `Type.method` calls,
/// by its key in a program, if its body is in `std.core`.
pub fn core_function(type_name: &str, method: &str) -> Option<String> {
    // `sort` changes its receiver: it is `list_sorted`, with the result
    // assigned back (`program/lower.rs`).
    let function = if type_name == "List" && method == "sort" {
        "sorted"
    } else {
        method
    };
    CORE_METHODS
        .iter()
        .find(|(ty, methods)| *ty == type_name && methods.contains(&method))
        .map(|_| format!("pkg.std.core.{}_{function}", type_name.to_lowercase()))
}

/// Whether `source` writes a call of a method named `name`: `.name`, with
/// blanks allowed after the dot. Errs toward yes.
fn calls_method(source: &str, name: &str) -> bool {
    let is_name_char = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices(name).any(|(at, _)| {
        !source[at + name.len()..]
            .chars()
            .next()
            .is_some_and(is_name_char)
            && source[..at].trim_end().ends_with('.')
    })
}

/// The names of module `path` that the prelude brings into every module.
pub fn prelude_names(path: &str) -> &'static [&'static str] {
    PRELUDE_STD
        .iter()
        .find(|(module, _)| *module == path)
        .map_or(&[], |(_, names)| names)
}

/// Whether `name` is a prelude name of module `path`.
pub fn is_prelude_name(path: &str, name: &str) -> bool {
    prelude_names(path).contains(&name)
}

/// Whether `source` writes the identifier `name`, in code, a comment or a
/// string: a cheap test that errs toward yes.
fn mentions(source: &str, name: &str) -> bool {
    let is_name_char = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices(name).any(|(at, _)| {
        !source[..at].chars().next_back().is_some_and(is_name_char)
            && !source[at + name.len()..]
                .chars()
                .next()
                .is_some_and(is_name_char)
    })
}

/// The `std` modules written in Oska that `module`, parsed from `source`,
/// uses: those it imports, in the order of their first `use`, then those
/// whose prelude names `source` writes ([`PRELUDE_STD`]).
pub fn oska_std_imports(module: &Module, source: &str) -> Vec<&'static StdModule> {
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
    for (path, names) in PRELUDE_STD {
        let Some(std) = std_module(path) else {
            continue;
        };
        if !found.iter().any(|m| m.path == std.path) && names.iter().any(|n| mentions(source, n)) {
            found.push(std);
        }
    }
    if !found.iter().any(|m| m.path == "std.core")
        && CORE_METHODS
            .iter()
            .any(|(_, names)| names.iter().any(|n| calls_method(source, n)))
    {
        if let Some(std) = std_module("std.core") {
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
                // A module with prelude names cannot be checked as a
                // file: its own declarations would clash with the
                // prelude names that the program it is joined to sees.
                // Programs that use it check it (`tests/map_prelude.rs`).
                if super::prelude_names(std.path).is_empty() {
                    let report = crate::check_module(std.source);
                    assert!(
                        report.diagnostics.is_empty(),
                        "{}: {:#?}",
                        std.path,
                        report.diagnostics
                    );
                }
                let ast = oska_syntax::parse_module(std.source).expect("the module parses");
                uses.extend(
                    super::oska_std_imports(&ast, std.source)
                        .iter()
                        .map(|m| m.path)
                        .filter(|path| *path != std.path),
                );
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
