//! The path-taking methods that `Fs`, `FsRead` and `FsWrite` no longer
//! have (ADR 0008). A call of one gets a dedicated diagnostic that
//! explains attenuation and shows the shape of the fix, in place of the
//! ordinary unknown-method error.

use oska_syntax::ast::{Arg, Expr};
use oska_syntax::Span;

use super::Body;
use crate::codes;
use crate::collect::DeclKind;
use crate::ty::Ty;

/// A removed method and what its fix looks like.
struct Removed {
    method: &'static str,
    /// Whether it writes: reads go through `DirRead`, writes through
    /// `DirWrite`.
    writes: bool,
    /// The arguments in the fix's shape.
    args: &'static str,
    /// Whether a file capability has the method too, with no path.
    on_files: bool,
}

const REMOVED: &[Removed] = &[
    Removed {
        method: "read_to_string",
        writes: false,
        args: "path",
        on_files: true,
    },
    Removed {
        method: "list_dir",
        writes: false,
        args: "path",
        on_files: false,
    },
    Removed {
        method: "write",
        writes: true,
        args: "path, text",
        on_files: true,
    },
    Removed {
        method: "create_dir",
        writes: true,
        args: "path",
        on_files: false,
    },
];

impl Body<'_, '_, '_> {
    /// Report the call at `span` of `base.name(...)` if `name` is a
    /// removed method of the receiver type: `FsRead` had the reads and
    /// `FsWrite` the writes. `Fs` never had them, but is told to narrow
    /// its access first. Returns whether it reported.
    pub(super) fn removed_path_method(
        &mut self,
        receiver: &Ty,
        base: &Expr,
        name: &str,
        args: &[Arg],
        span: Span,
    ) -> bool {
        let Ty::Named(type_name, _, _) = receiver else {
            return false;
        };
        let is_prelude_capability = self
            .decls()
            .get(type_name)
            .is_some_and(|d| d.in_prelude && d.kind == DeclKind::Capability);
        let Some(removed) = REMOVED.iter().find(|r| r.method == name) else {
            return false;
        };
        let applies = match type_name.as_str() {
            "FsRead" => !removed.writes,
            "FsWrite" => removed.writes,
            "Fs" => true,
            _ => false,
        };
        if !(is_prelude_capability && applies) {
            return false;
        }
        let receiver_text = self.postfix_operand(base);
        let narrowed = match (type_name.as_str(), removed.writes) {
            ("Fs", false) => format!("{receiver_text}.read_only()"),
            ("Fs", true) => format!("{receiver_text}.write_only()"),
            _ => receiver_text,
        };
        let mut message = format!(
            "`{type_name}` cannot {} files itself, because it only attenuates. Attenuate it to \
             a directory first and call `{name}` on that: \
             `{narrowed}.restrict_to(\"<root>\").{name}({})`, where the root is the directory \
             the program should reach",
            if removed.writes { "write" } else { "read" },
            removed.args,
        );
        if removed.on_files {
            message.push_str(&format!(
                ". For a single file, use `restrict_to_file(\"<file>\")` and call `{name}` \
                 with no path"
            ));
        }
        self.error(codes::PATH_METHOD_ON_FS, message, span);
        self.infer_args(args);
        true
    }
}
