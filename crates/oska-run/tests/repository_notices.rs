// SPDX-License-Identifier: Apache-2.0
//! The files a reviewer expects at the top of an Apache 2.0 repository
//! (spec #416, ticket #545): `NOTICE` (license section 4), `CODEOWNERS`,
//! and `CONTRIBUTING.md` (inbound license, section 5; points to
//! `SECURITY.md`).

mod common;

use common::site::repository_root;

fn read(path: &str) -> String {
    let file = repository_root().join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()))
}

#[test]
fn notice_names_the_project_and_its_copyright_holder_and_the_license() {
    let notice = read("NOTICE");
    assert!(notice.starts_with("Oska\n"), "{notice}");
    let holder = read("LICENSE")
        .lines()
        .find_map(|l| l.trim().strip_prefix("Copyright 2026 ").map(str::to_owned))
        .expect("LICENSE carries a copyright line");
    assert!(
        notice.contains(&format!("Copyright 2026 {holder}")),
        "{notice}"
    );
    assert!(notice.contains("Apache License, Version 2.0"), "{notice}");
}

#[test]
fn codeowners_assigns_every_path_to_an_owner() {
    let owners = read(".github/CODEOWNERS");
    let rules: Vec<&str> = owners
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let catch_all = rules
        .iter()
        .find(|r| r.starts_with("* "))
        .unwrap_or_else(|| panic!("no catch-all rule in {owners}"));
    assert!(catch_all
        .split_whitespace()
        .skip(1)
        .all(|o| o.starts_with('@')));
    assert!(catch_all.split_whitespace().count() >= 2, "{catch_all}");
}

#[test]
fn contributing_states_the_inbound_license_and_links_the_security_policy() {
    let text = read("CONTRIBUTING.md");
    assert!(text.contains("Apache License, Version 2.0"), "{text}");
    assert!(text.contains("section 5"), "{text}");
    assert!(text.contains("](SECURITY.md)"), "{text}");
    assert!(
        repository_root().join("SECURITY.md").is_file(),
        "the linked policy exists"
    );
    assert!(text.contains("](NOTICE)"), "{text}");
}
