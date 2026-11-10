//! What the checker knows about the prelude's built-in types and traits,
//! for backends: the one source of which types are capabilities, which
//! `std` modules exist, what a `TestContext` holds, and the names of the
//! types and trait methods a backend treats specially. The integer types
//! are [`crate::program::IntType`].
//!
//! Everything here is read from the prelude and `std` sources the checker
//! embeds, or is one of the checker's own name constants, so a backend
//! never keeps a list of its own that could drift from the language.

use std::sync::OnceLock;

use oska_syntax::ast::{ItemKind, TypeKind};

use crate::collect::field_types;
use crate::prelude;

pub use crate::ty::{
    ARGS, BOOL, DISPLAY, EQ, FROM, INT, LIST, OPTION, RANGE, RESULT, SCOPE, SECRET, STRING,
    TEST_CONTEXT,
};

/// The prelude's `Task`, which `Scope.spawn` returns.
pub const TASK: &str = "Task";

/// The prelude's `Path`.
pub const PATH: &str = "Path";
/// The prelude's `IoError`, the error of every I/O operation.
pub const IO_ERROR: &str = "IoError";

/// The variants of `Option` and `Result`.
pub const SOME: &str = "Some";
pub const NONE: &str = "None";
pub const OK: &str = "Ok";
pub const ERR: &str = "Err";

/// A trait's method, by name: what generic code calls on a value whose
/// type only the runtime knows (`program::Callee::Trait`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitMethod {
    pub trait_name: &'static str,
    pub method: &'static str,
}

/// `Eq.eq`: `==` on a type that is not built in.
pub const EQ_EQ: TraitMethod = TraitMethod {
    trait_name: EQ,
    method: "eq",
};

/// `Display.to_string`: interpolation, and `main`'s error on failure.
pub const DISPLAY_TO_STRING: TraitMethod = TraitMethod {
    trait_name: DISPLAY,
    method: "to_string",
};

/// The name of every built-in capability, the prelude's structs marked
/// `#[capability]`, in declaration order.
pub fn capabilities() -> &'static [&'static str] {
    static CAPABILITIES: OnceLock<Vec<&'static str>> = OnceLock::new();
    CAPABILITIES.get_or_init(|| {
        prelude::module()
            .items
            .iter()
            .filter(|item| item.attributes.iter().any(|a| a.name.name == "capability"))
            .filter_map(prelude::item_name)
            .collect()
    })
}

/// Whether the type named `name` is a built-in capability.
pub fn is_capability(name: &str) -> bool {
    capabilities().contains(&name)
}

/// The path of every `std` module a program can import, such as
/// `std.sql`.
pub fn std_modules() -> impl Iterator<Item = &'static str> {
    prelude::STD_MODULES.iter().map(|m| m.path)
}

/// The `std` module that the imported path `path` is in: `std.sql` for
/// `std.sql` itself or for `std.sql.Db`.
pub fn std_module_of(path: &str) -> Option<&'static str> {
    std_modules().find(|m| {
        path == *m
            || path
                .strip_prefix(m)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// The type of each field of the prelude's `TestContext`, `t` in a test,
/// in declaration order: its built-in capabilities, such as `Fs`.
pub fn test_context_fields() -> &'static [&'static str] {
    static FIELDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        let decl = prelude::module()
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Struct(s) if s.name.name == TEST_CONTEXT => Some(s),
                _ => None,
            })
            .expect("the prelude declares `TestContext`");
        field_types(&decl.fields)
            .into_iter()
            .map(|ty| match &ty.kind {
                TypeKind::Path { segments, .. } => segments
                    .last()
                    .map(|s| s.name.as_str())
                    .expect("a path has a segment"),
                other => panic!("`TestContext` holds a field of type {other:?}"),
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_are_the_preludes_marked_structs() {
        let found = capabilities();
        for expected in ["Stdin", "Stdout", "Fs", "FsRead", "Net", "Spawn", "Sched"] {
            assert!(found.contains(&expected), "{expected} in {found:?}");
        }
        assert!(!found.contains(&"Connection"), "a `cap struct` is not one");
        assert!(!found.contains(&ARGS));
    }

    #[test]
    fn a_path_names_the_std_module_it_is_in() {
        assert_eq!(std_module_of("std.sql"), Some("std.sql"));
        assert_eq!(std_module_of("std.sql.Db"), Some("std.sql"));
        assert_eq!(std_module_of("std.sqlite"), None);
    }

    #[test]
    fn a_test_context_holds_its_capabilities_in_order() {
        assert_eq!(test_context_fields(), ["Fs", "Clock", "Random", "Stdout"]);
    }
}
