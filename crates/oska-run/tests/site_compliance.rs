// SPDX-License-Identifier: Apache-2.0
//! The Compliance section on the site (#547, spec #416).
//!
//! `docs/compliance/` is published like any other docs section (ADR 0006).
//! When it exists, the Security and Reference pages link to its index and
//! each of its pages has a "Download PDF" link to the same path with `.pdf`.
//! When it does not, nothing mentions it.
//!
//! The PDFs are printed by the Pages workflow with headless Chrome. A local
//! build has none, so the link checker accepts a missing `.pdf` beside its
//! `.html` only when the site holds the file `pdf-skipped`; the Pages
//! workflow never writes it, so a missing PDF there fails the build.

mod common;

use std::collections::BTreeMap;

use common::site::{self, check_links, succeeded, MINIMAL_REPO};

const COMPLIANCE: &[(&str, &str)] = &[
    // The pages `llms.txt` always links.
    ("docs/guide/packages.md", "# Packages\n"),
    (
        "docs/guide/cli-reference.md",
        "# CLI\n\n## Platform support\n",
    ),
    ("SECURITY.md", "# Security\n\nReport a vulnerability.\n"),
    (
        "docs/compliance/index.md",
        "# Compliance\n\nThe reports.\n\n- [Apache](apache.md)\n",
    ),
    (
        "docs/compliance/apache.md",
        "# Apache License 2.0\n\nConforms.\n",
    ),
];

fn site_of(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    site::site_of(MINIMAL_REPO, inputs)
}

#[test]
fn with_the_folder_the_section_security_and_reference_link_it_and_each_page_has_a_pdf_link() {
    let site = site_of(COMPLIANCE);
    for page in ["compliance/index", "compliance/apache"] {
        let html = &site[&format!("{page}.html")];
        assert!(
            html.contains(&format!("<a href=\"/{page}.pdf\">Download PDF</a>")),
            "{page} has no PDF link: {html}"
        );
        let md = &site[&format!("{page}.md")];
        assert!(md.contains(&format!("[Download PDF](/{page}.pdf)")), "{md}");
    }
    assert!(
        site["security.html"].contains("<a href=\"/compliance/\">Compliance reports</a>"),
        "{}",
        site["security.html"]
    );
    assert!(
        site["security.md"].contains("(/compliance/index.md)")
            || site["security.md"].contains("compliance/index.md"),
        "{}",
        site["security.md"]
    );
    assert!(
        site["reference.html"].contains("<a href=\"/compliance/\">compliance</a>"),
        "{}",
        site["reference.html"]
    );
}

#[test]
fn without_the_folder_the_section_is_absent_and_unlinked() {
    let site = site_of(&[("SECURITY.md", "# Security\n\nReport a vulnerability.\n")]);
    assert!(!site.keys().any(|k| k.starts_with("compliance/")));
    for page in [
        "security.html",
        "security.md",
        "reference.html",
        "reference.md",
        "index.html",
    ] {
        assert!(
            !site[page].to_lowercase().contains("compliance"),
            "{page} mentions compliance: {}",
            site[page]
        );
    }
    assert!(!site.values().any(|text| text.contains("Download PDF")));
}

#[test]
fn other_pages_get_no_pdf_link() {
    let site = site_of(COMPLIANCE);
    assert!(!site["design.html"].contains(".pdf"));
    assert!(!site["security.html"].contains(".pdf"));
}

#[test]
fn the_top_navigation_does_not_change() {
    let with = site_of(COMPLIANCE);
    let rest: Vec<(&str, &str)> = COMPLIANCE
        .iter()
        .copied()
        .filter(|(path, _)| !path.starts_with("docs/compliance/"))
        .collect();
    let without = site_of(&rest);
    assert_eq!(
        site::navigation(&with["index.html"]),
        site::navigation(&without["index.html"])
    );
}

#[test]
fn a_missing_pdf_fails_the_link_check_unless_the_pdf_step_was_skipped() {
    let site = site_of(COMPLIANCE);
    let outcome = check_links(&site, &[]);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains("the site has no compliance/apache.pdf"),
        "{}",
        outcome.stderr
    );

    let mut skipped = site.clone();
    skipped.insert("pdf-skipped".to_string(), "\n".to_string());
    let outcome = check_links(&skipped, &[]);
    assert!(succeeded(&outcome), "{outcome:#?}");

    // The flag excuses a PDF only beside its page, and only a PDF.
    let mut stray = skipped.clone();
    stray
        .get_mut("design.html")
        .expect("design.html")
        .push_str("<a href=\"/nowhere.pdf\">x</a>\n<a href=\"/gone.html\">y</a>\n");
    let outcome = check_links(&stray, &[]);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    assert!(
        outcome.stderr.contains("the site has no nowhere.pdf"),
        "{}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("the site has no gone.html"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn the_link_check_passes_when_the_pdfs_exist() {
    let mut site = site_of(COMPLIANCE);
    for page in ["compliance/index", "compliance/apache"] {
        site.insert(format!("{page}.pdf"), "%PDF\n".to_string());
    }
    let outcome = check_links(&site, &[]);
    assert!(succeeded(&outcome), "{outcome:#?}");
}
