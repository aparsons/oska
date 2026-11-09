//! Audit sites: calls a reviewer must see, listed in the report's `audit`
//! array.
//!
//! An audit site is a call to one of the prelude methods in [`AUDITED`],
//! recorded where it is called. To add a kind, add its method to the
//! table: the site's kind is the method's name.

use oska_syntax::Span;

use super::Body;
use crate::ty;
use crate::AuditSite;

/// Each audited method: the type that declares it, and its name, which is
/// also the kind of its audit sites.
const AUDITED: &[(&str, &str)] = &[
    // A trust assertion: vouches for untrusted text.
    (ty::STRING, "assume_trusted"),
];

impl Body<'_, '_, '_> {
    /// Record the call at `span` of the method `method` on a value of the
    /// type named `type_name`, if it is audited.
    pub(super) fn audit_call(&mut self, type_name: &str, method: &str, span: Span) {
        if let Some(&(_, kind)) = AUDITED
            .iter()
            .find(|(ty, name)| *ty == type_name && *name == method)
        {
            self.audit.push(AuditSite { kind, span });
        }
    }
}
