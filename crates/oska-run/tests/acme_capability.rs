// SPDX-License-Identifier: Apache-2.0
//! The `Acme` capability at run time (spec #824, ticket #840): a program
//! holding `Acme.restrict_to("example.com")` may order `example.com` and
//! `www.example.com`, and `std.acme` refuses any other name before a
//! request is made, with the precondition panic `P0006`. Each program runs
//! on the interpreter and, where native code runs, natively
//! (`common/native.rs`), and the two must agree.

mod common;

use common::run_fake;

/// A program that asks `permits` of each name, printing `yes` or `no`.
fn permits(names: &[&str]) -> String {
    let checks: String = names
        .iter()
        .map(|n| {
            format!("    let answer = if acme.permits(domain, \"{n}\") {{ \"true\" }} else {{ \"false\" }}\n    let _ = out.println(\"{n}: {{answer}}\")\n")
        })
        .collect();
    format!(
        "use std.acme\n\nfn main(grant: Acme, out: Stdout) {{\n    let domain = grant.restrict_to(\"example.com\")\n{}}}\n",
        checks
    )
}

#[test]
fn the_domain_and_names_beneath_it_are_permitted_and_others_are_not() {
    let outcome = run_fake(
        &permits(&[
            "example.com",
            "www.example.com",
            "a.b.example.com",
            "EXAMPLE.com",
            "*.example.com",
            "other.org",
            "notexample.com",
            "example.com.evil.org",
            "",
            ".example.com",
            "*.other.org",
        ]),
        &[],
    );
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{outcome:#?}"
    );
    assert_eq!(
        outcome.stdout,
        "example.com: true\nwww.example.com: true\na.b.example.com: true\nEXAMPLE.com: true\n\
         *.example.com: true\nother.org: false\nnotexample.com: false\nexample.com.evil.org: false\n\
         : false\n.example.com: false\n*.other.org: false\n"
    );
}

#[test]
fn a_domain_that_is_not_a_plain_name_permits_nothing() {
    for bad in ["", "*.example.com", ".example.com", "example.com.", "a b"] {
        let src = format!(
            "use std.acme\n\nfn main(grant: Acme, out: Stdout) {{\n    let domain = grant.restrict_to(\"{bad}\")\n    let answer = if acme.permits(domain, \"example.com\") {{ \"true\" }} else {{ \"false\" }}\n    let _ = out.println(\"{{answer}}\")\n}}\n"
        );
        let outcome = run_fake(&src, &[]);
        assert_eq!(outcome.stdout, "false\n", "{bad:?}: {outcome:#?}");
    }
}

#[test]
fn the_domain_reads_back_as_given() {
    let outcome = run_fake(
        "fn main(grant: Acme, out: Stdout) {\n    let _ = out.println(grant.restrict_to(\"Example.com\").domain())\n}\n",
        &[],
    );
    assert_eq!(outcome.stdout, "Example.com\n", "{outcome:#?}");
}

#[test]
fn an_order_for_a_permitted_name_goes_ahead() {
    let outcome = run_fake(
        "use std.acme\n\nfn main(grant: Acme, out: Stdout) {\n    let domain = grant.restrict_to(\"example.com\")\n    let names = acme.require_names(domain, [\"example.com\", \"www.example.com\"])\n    let count = names.len()\n    let _ = out.println(\"{count}\")\n}\n",
        &[],
    );
    assert_eq!(outcome.stdout, "2\n", "{outcome:#?}");
}

#[test]
fn an_order_for_a_name_outside_the_domain_panics_before_any_request() {
    let outcome = run_fake(
        "use std.acme\n\nfn main(grant: Acme) {\n    let domain = grant.restrict_to(\"example.com\")\n    let _ = acme.require_names(domain, [\"example.com\", \"other.org\"])\n}\n",
        &[],
    );
    let panic = outcome
        .panic
        .as_ref()
        .unwrap_or_else(|| panic!("{outcome:#?}"));
    assert_eq!(panic.code, "P0006", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 101);
}

#[test]
fn main_cannot_be_given_a_narrowed_domain() {
    let outcome = run_fake("fn main(d: AcmeDomain) {\n}\n", &[]);
    assert!(
        outcome.diagnostics.iter().any(|d| d.code == "E0217"),
        "{outcome:#?}"
    );
}
