//! The authority diff: what one check report's program can do that
//! another's could not. `oskac fix` refuses to write a fixed program that
//! this reports any widening for.
//!
//! Functions are keyed by their report name, so a rename is a removal and
//! an addition, never guessed as the same function. Authority entries are
//! compared as text: an entry the old function did not list is a
//! widening, even a kind its old entries could be attenuated into.
//! Audit sites are counted per file and kind, since the report does not
//! name the function they are in.

use std::collections::{BTreeMap, BTreeSet};

use oska_check::CheckReport;

/// The widening of a program's authority from one report to another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityDiff {
    /// Functions in both reports, each with the entries it gained.
    pub widened: Vec<(String, Vec<String>)>,
    /// Functions that were pure and are not.
    pub became_impure: Vec<String>,
    /// Functions only in the new report, each with the entries of its
    /// authority that no function of the old report had.
    pub added: Vec<(String, Vec<String>)>,
    /// Audit sites, as (file, kind), that there are more of, with how many
    /// more.
    pub new_audit_sites: Vec<(String, String, usize)>,
}

/// What `after`'s program can do that `before`'s could not.
pub fn diff(before: &CheckReport, after: &CheckReport) -> AuthorityDiff {
    let mut found = AuthorityDiff::default();
    let program_had: BTreeSet<&String> =
        before.functions.iter().flat_map(|f| &f.authority).collect();
    for function in &after.functions {
        match before.functions.iter().find(|f| f.name == function.name) {
            Some(old) => {
                let gained: Vec<String> = function
                    .authority
                    .iter()
                    .filter(|entry| !old.authority.contains(entry))
                    .cloned()
                    .collect();
                if !gained.is_empty() {
                    found.widened.push((function.name.clone(), gained));
                }
                if old.pure && !function.pure {
                    found.became_impure.push(function.name.clone());
                }
            }
            None => {
                let new_kinds: Vec<String> = function
                    .authority
                    .iter()
                    .filter(|entry| !program_had.contains(entry))
                    .cloned()
                    .collect();
                if !new_kinds.is_empty() {
                    found.added.push((function.name.clone(), new_kinds));
                }
            }
        }
    }
    let (old_sites, new_sites) = (audit_counts(before), audit_counts(after));
    for ((path, kind), count) in new_sites {
        let had = old_sites
            .get(&(path.clone(), kind.clone()))
            .copied()
            .unwrap_or(0);
        if count > had {
            found.new_audit_sites.push((path, kind, count - had));
        }
    }
    found
}

/// How many audit sites `report` has, per file and kind.
fn audit_counts(report: &CheckReport) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for site in &report.audit {
        let path = site
            .location
            .as_ref()
            .map_or_else(String::new, |l| l.path.clone());
        *counts
            .entry((path, site.kind.as_str().to_string()))
            .or_insert(0) += 1;
    }
    counts
}

impl AuthorityDiff {
    /// Each widening, said in a few words, such as "`fn q` gains
    /// `NetConnect`": empty when there is none.
    pub fn widenings(&self) -> Vec<String> {
        let list = |entries: &[String]| {
            entries
                .iter()
                .map(|e| format!("`{e}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut out = Vec::new();
        for (name, gained) in &self.widened {
            out.push(format!("`fn {name}` gains {}", list(gained)));
        }
        for name in &self.became_impure {
            out.push(format!("`fn {name}` is no longer pure"));
        }
        for (name, kinds) in &self.added {
            out.push(format!(
                "the new `fn {name}` reaches {}, which no function did",
                list(kinds)
            ));
        }
        for (path, kind, more) in &self.new_audit_sites {
            let sites = if *more == 1 { "site" } else { "sites" };
            out.push(format!("{more} new `{kind}` audit {sites} in `{path}`"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::diff;
    use oska_check::{AuditKind, AuditSite, CheckReport, FunctionAuthority};
    use oska_syntax::{Location, Span};

    fn function(name: &str, authority: &[&str]) -> FunctionAuthority {
        FunctionAuthority {
            name: name.to_string(),
            public: false,
            authority: authority.iter().map(|a| a.to_string()).collect(),
            via: Vec::new(),
            pure: authority.is_empty(),
        }
    }

    fn report(functions: Vec<FunctionAuthority>, audit: &[AuditKind]) -> CheckReport {
        CheckReport {
            functions,
            audit: audit
                .iter()
                .map(|kind| AuditSite {
                    kind: *kind,
                    span: Span::at(0),
                    location: Some(Location::new("app.osk", "", 0)),
                })
                .collect(),
            ..CheckReport::default()
        }
    }

    #[test]
    fn the_same_authority_is_no_widening() {
        let before = report(vec![function("main", &["Stdout"])], &[]);
        assert_eq!(
            diff(&before, &before.clone()).widenings(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_function_that_gains_an_entry_or_becomes_impure_widens() {
        let before = report(vec![function("main", &["Stdout"]), function("q", &[])], &[]);
        let after = report(
            vec![
                function("main", &["Stdout", "Net"]),
                function("q", &["Clock"]),
            ],
            &[],
        );
        assert_eq!(
            diff(&before, &after).widenings(),
            [
                "`fn main` gains `Net`",
                "`fn q` gains `Clock`",
                "`fn q` is no longer pure"
            ]
        );
    }

    #[test]
    fn a_new_function_widens_only_with_a_kind_the_program_never_had() {
        let before = report(vec![function("main", &["Stdout"])], &[]);
        let after = report(
            vec![
                function("main", &["Stdout"]),
                function("helper", &["Stdout"]),
                function("fetch", &["Net"]),
            ],
            &[],
        );
        assert_eq!(
            diff(&before, &after).widenings(),
            ["the new `fn fetch` reaches `Net`, which no function did"]
        );
    }

    #[test]
    fn narrowing_and_removal_are_not_widenings() {
        let before = report(
            vec![function("main", &["Stdout", "Net"]), function("old", &[])],
            &[],
        );
        let after = report(vec![function("main", &["Stdout"])], &[]);
        assert_eq!(diff(&before, &after).widenings(), Vec::<String>::new());
    }

    #[test]
    fn a_new_audit_site_widens_and_a_removed_one_does_not() {
        let one = report(vec![], &[AuditKind::TrustAssertion]);
        let two = report(
            vec![],
            &[AuditKind::TrustAssertion, AuditKind::TrustAssertion],
        );
        assert_eq!(
            diff(&one, &two).widenings(),
            ["1 new `assume_trusted` audit site in `app.osk`"]
        );
        assert_eq!(diff(&two, &one).widenings(), Vec::<String>::new());
    }
}
