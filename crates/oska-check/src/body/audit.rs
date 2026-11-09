//! Audit sites: calls a reviewer must see, listed in the report's `audit`
//! array.
//!
//! An audit site is a call to one of the prelude methods in [`AUDITED`],
//! recorded where it is called. To add a kind, add a variant to
//! [`AuditKind`] and its method to the table.

use oska_syntax::Span;

use super::Body;
use crate::ty;
use crate::{AuditKind, AuditSite};

/// An audited method: the type that declares it, its name, and the kind
/// of its audit sites.
struct Audited {
    type_name: &'static str,
    method: &'static str,
    kind: AuditKind,
}

const AUDITED: &[Audited] = &[
    Audited {
        type_name: ty::STRING,
        method: "assume_trusted",
        kind: AuditKind::TrustAssertion,
    },
    Audited {
        type_name: ty::SECRET,
        method: "expose",
        kind: AuditKind::SecretExposure,
    },
];

impl Body<'_, '_, '_> {
    /// Record the call at `span` of the method `method` on a value of the
    /// type named `type_name`, if it is audited.
    pub(super) fn audit_call(&mut self, type_name: &str, method: &str, span: Span) {
        if let Some(audited) = AUDITED
            .iter()
            .find(|a| a.type_name == type_name && a.method == method)
        {
            self.out.audit.push(AuditSite {
                kind: audited.kind,
                span,
            });
        }
    }
}
