//! The Evaluation page (#216): `docs/evaluation.md`, which the evaluation
//! harness renders from the committed results (`oska-eval report`; its
//! own tests hold the page to the results), is published like any other
//! top-level doc: as `evaluation.html` with a Markdown copy, listed on the
//! Reference page, and in `llms-full.txt`.
//!
//! The generator runs through the interpreter on a `FakeHost`, over the
//! repository's real docs (`common`).

mod common;

use common::site::real_site;

#[test]
fn the_real_site_publishes_the_evaluation_page_from_the_reference_page() {
    let site = real_site();
    let page = site
        .get("evaluation.html")
        .expect("the site publishes docs/evaluation.md");
    assert!(page.contains("<h1"), "{page}");
    assert!(page.contains("Evaluation"), "{page}");
    assert!(
        page.contains("Limitations"),
        "the page states its limitations"
    );
    assert!(
        site.contains_key("evaluation.md"),
        "the page has a Markdown copy"
    );
    let reference = site.get("reference.html").expect("a Reference page");
    assert!(
        reference.contains("evaluation.html"),
        "the Reference page links the Evaluation page: {reference}"
    );
    let full = site.get("llms-full.txt").expect("llms-full.txt");
    assert!(
        full.contains("# Evaluation"),
        "llms-full.txt holds the page"
    );
}
