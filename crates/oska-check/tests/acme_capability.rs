// SPDX-License-Identifier: Apache-2.0
//! The `Acme` capability (spec #824, ticket #840): it only attenuates, as
//! `Net` does (ADR 0025). `Acme.restrict_to(domain)` gives an
//! `AcmeDomain`, which names the one domain a program may get certificates
//! for, and the authority report lists each narrowing with its domain.

mod common;

use common::{assert_errors, assert_ok, check};
use oska_check::authority_diff::covers;
use oska_check::AttenuationRoot::{Computed, Literal};

#[test]
fn main_may_declare_acme_and_narrow_it_to_a_domain() {
    assert_ok(
        "fn main(acme: Acme) {\n    let domain = acme.restrict_to(\"example.com\")\n    let _ = domain.domain()\n}",
    );
}

#[test]
fn a_domain_narrowing_is_listed_with_its_domain() {
    let src = "fn names(acme: Acme) -> AcmeDomain {\n    acme.restrict_to(\"example.com\")\n}";
    let found = check(src).attenuations;
    assert_eq!(found.len(), 1, "{found:?}");
    let a = &found[0];
    assert_eq!(a.function.to_string(), "names");
    assert_eq!(a.method, "restrict_to");
    assert_eq!(
        (a.source.as_str(), a.result.as_str()),
        ("Acme", "AcmeDomain")
    );
    assert_eq!(a.root, Literal("example.com".to_string()));
    assert_eq!(
        &src[a.span.start..a.span.end],
        "acme.restrict_to(\"example.com\")"
    );
}

#[test]
fn a_domain_known_only_at_run_time_is_listed_as_computed() {
    let found =
        check("fn pick(acme: Acme, name: String) -> AcmeDomain {\n    acme.restrict_to(name)\n}")
            .attenuations;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].root, Computed);
}

#[test]
fn a_domain_named_by_a_constant_is_the_constants_text() {
    let found = check(
        "const SITE: String = \"example.com\"\nfn names(acme: Acme) -> AcmeDomain {\n    acme.restrict_to(SITE)\n}",
    )
    .attenuations;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].root, Literal("example.com".to_string()));
}

#[test]
fn main_cannot_declare_a_narrowed_domain_capability() {
    assert_errors(
        "fn main(domain: AcmeDomain) {\n}",
        &[("E0217", "AcmeDomain")],
    );
}

#[test]
fn untrusted_text_cannot_choose_the_domain() {
    assert_errors(
        "fn names(acme: Acme, name: Untrusted<String>) -> AcmeDomain {\n    acme.restrict_to(name)\n}",
        &[("E0300", "name")],
    );
}

#[test]
fn acme_covers_its_narrowing_but_not_the_other_way() {
    assert!(covers("Acme", "AcmeDomain"));
    assert!(!covers("AcmeDomain", "Acme"));
    assert!(!covers("Acme", "NetConnect"));
    assert!(!covers("Net", "AcmeDomain"));
    assert!(!covers("AcmeDomain", "HostConnect"));
}

#[test]
fn acme_is_one_of_the_built_in_capabilities() {
    assert!(oska_check::builtins::is_capability("Acme"));
    assert!(oska_check::builtins::is_capability("AcmeDomain"));
}
