//! Attenuations: calls that narrow a capability, listed in the report's
//! `attenuations` array.
//!
//! An attenuation is a call of one of the prelude's attenuation methods
//! ([`METHODS`]) on a built-in capability, returning a built-in capability.
//! Recording them by this rule, and not by listing each kind, means a new
//! capability kind with such a method shows up with no change here.

use oska_syntax::ast::{Arg, Expr, ExprKind};
use oska_syntax::Span;

use super::Body;
use crate::builtins;
use crate::collect::DeclKind;
use crate::items::{literal_text, FnSig};
use crate::{Attenuation, AttenuationRoot};

/// The attenuation methods, and whether each takes a root as its first
/// argument.
const METHODS: &[(&str, bool)] = &[
    ("restrict_to", true),
    ("restrict_to_file", true),
    ("read_only", false),
    ("write_only", false),
];

impl Body<'_, '_, '_> {
    /// Record the call at `span` of the method `sig` on a value of the
    /// type named `type_name`, if it attenuates a capability.
    pub(super) fn attenuation_call(
        &mut self,
        type_name: &str,
        sig: &FnSig,
        args: &[Arg],
        span: Span,
    ) {
        let Some(&(method, takes_root)) = METHODS.iter().find(|(name, _)| *name == sig.name) else {
            return;
        };
        let Some(result) = sig.ret.head() else {
            return;
        };
        if !(self.is_builtin_capability(type_name) && self.is_builtin_capability(result)) {
            return;
        }
        let root = if takes_root {
            match args.first().and_then(|arg| self.root_text(&arg.value)) {
                Some(text) => AttenuationRoot::Literal(text),
                None => AttenuationRoot::Computed,
            }
        } else {
            AttenuationRoot::Absent
        };
        let attenuation = Attenuation {
            function: self.out.function.clone(),
            method: method.to_string(),
            span,
            source: type_name.to_string(),
            result: result.to_string(),
            root,
        };
        self.out.attenuations.push(attenuation);
    }

    fn is_builtin_capability(&self, name: &str) -> bool {
        self.decls()
            .get(name)
            .is_some_and(|d| d.in_prelude && d.kind == DeclKind::Capability)
    }

    /// The text of a root argument that is known before the program runs:
    /// a string literal, a module constant holding one, or `Path.new` of
    /// either. `None` for anything else, such as a variable, an
    /// interpolated string, or `Path.from(untrusted)`.
    fn root_text(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Str(_) => literal_text(expr),
            ExprKind::Name(ident) if self.local(&ident.name).is_none() => {
                self.items.consts.get(&ident.name).cloned()
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Field { base, name } = &callee.kind else {
                    return None;
                };
                let ExprKind::Name(path) = &base.kind else {
                    return None;
                };
                let is_path_new = path.name == builtins::PATH
                    && name.name == "new"
                    && self.local(&path.name).is_none();
                match args.as_slice() {
                    [arg] if is_path_new => self.root_text(&arg.value),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
