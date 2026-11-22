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

// ---- The real section (#548) -------------------------------------------------------------
//
// The index, the FAQ and the report template, built from the repository's
// own `docs/compliance/`.

const STANDARDS: [&str; 10] = [
    "Apache License 2.0",
    "NIST SP 800-218",
    "OpenSSF Open Source Project Security Baseline",
    "SLSA",
    "SPDX",
    "ISO/IEC 25010",
    "NIST Cybersecurity Framework 2.0",
    "OpenSSF Scorecard",
    "ISO/IEC 5230",
    "ISO/IEC 18974",
];

fn doc(name: &str) -> String {
    let path = site::repository_root().join("docs/compliance").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Whole words, so that "certified" is not "certification".
fn has_word(text: &str, word: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == word)
}

#[test]
fn the_index_lists_all_ten_standards() {
    let index = doc("index.md");
    for standard in STANDARDS {
        assert!(index.contains(standard), "the index lacks {standard}");
    }
    let page = &site::real_site()["compliance/index.html"];
    for standard in STANDARDS {
        assert!(page.contains(standard), "the page lacks {standard}");
    }
}

#[test]
fn the_faq_links_evidence_for_every_answer_and_gives_no_legal_advice() {
    let faq = doc("faq.md");
    let answers: Vec<&str> = faq.split("\n## ").skip(1).collect();
    assert!(
        answers.len() >= 5,
        "license, SBOM, signatures, provenance, vulnerability"
    );
    for answer in &answers {
        assert!(
            answer.contains("](") && (answer.contains("https://") || answer.contains("../")),
            "an answer has no evidence link: {answer}"
        );
    }
    let lower = faq.to_lowercase();
    assert!(lower.contains("own counsel"), "{faq}");
    assert!(lower.contains("no legal advice"), "{faq}");
    assert!(lower.contains("export"), "{faq}");
    assert!(
        faq.contains("Conforms"),
        "the FAQ explains the Conforms rule"
    );
    assert!(lower.contains("low-rated"), "{faq}");
}

#[test]
fn the_template_fixes_the_sections_the_scale_and_the_determination_rule() {
    let template = doc("template.md");
    for heading in [
        "Executive summary",
        "Scope",
        "Determination",
        "Findings with recommendations",
        "Observations with recommendations",
        "Conclusions",
    ] {
        assert!(
            template.contains(&format!("\n### {heading}\n")),
            "the template lacks the section {heading}"
        );
    }
    for word in ["High", "Medium", "Low", "likelihood", "impact"] {
        assert!(template.contains(word), "the template lacks {word}");
    }
    for word in ["Conforms", "Partially conforms", "Does not conform"] {
        assert!(template.contains(word), "the template lacks {word}");
    }
    assert!(template.contains("self-assessment"), "{template}");
    assert!(template.contains("Finding") && template.contains("Observation"));
    assert!(
        template.contains("can sit beside a Low-rated"),
        "the rule that Conforms can sit beside a Low-rated unmet requirement"
    );
    assert!(template.contains("no third party"), "{template}");
}

#[test]
fn the_compliance_pages_use_the_glossary_terms_and_name_the_project_as_author() {
    for name in ["index.md", "faq.md", "template.md"] {
        let text = doc(name);
        for banned in ["audit", "audits", "certification", "attestation"] {
            assert!(!has_word(&text, banned), "{name} says {banned}");
        }
        for person in ["claude", "anthropic", "adam", "parsons", "agent", "ai"] {
            assert!(!has_word(&text, person), "{name} names {person}");
        }
        assert!(text.contains("Oska project"), "{name} names no author");
    }
}

#[test]
fn the_real_section_is_built_with_pdf_links_and_no_dead_link() {
    let mut site = site::real_site();
    for page in ["compliance/index", "compliance/faq", "compliance/template"] {
        let html = site
            .get(&format!("{page}.html"))
            .unwrap_or_else(|| panic!("no {page}.html"));
        assert!(html.contains(&format!("href=\"/{page}.pdf\"")), "{page}");
        assert!(site.contains_key(&format!("{page}.md")), "{page}.md");
    }
    assert!(site["security.html"].contains("href=\"/compliance/\""));
    site.insert("pdf-skipped".to_string(), "\n".to_string());
    let tracked = site::tracked_files();
    let tracked: Vec<&str> = tracked.iter().map(String::as_str).collect();
    let outcome = check_links(&site, &tracked);
    assert!(succeeded(&outcome), "{outcome:#?}");
}
