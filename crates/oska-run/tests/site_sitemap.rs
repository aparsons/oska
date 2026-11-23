// SPDX-License-Identifier: Apache-2.0
//! The sitemap's `<lastmod>` (#293): each page with a source file is
//! dated from `site/lastmod.txt`, which the Pages workflow writes from git
//! before the build, and a line that is not a real `YYYY-MM-DD` date is
//! skipped.
//!
//! The generator runs through the interpreter on a [`FakeHost`], and
//! natively on aarch64 and x86_64 Linux (`common`). Every page a test builds
//! goes through the same inertness check as in `site.rs`.

mod common;

use common::site::{agent_repo, build_ok, PAGES_WORKFLOW};

#[test]
fn the_sitemap_dates_each_page_with_a_source_file_from_lastmod_txt() {
    let mut repo = agent_repo();
    repo.push((
        "site/lastmod.txt",
        "2026-11-03 docs/index.md\n\
         2026-01-09 GLOSSARY.md\n\
         2026-1-09 docs/design.md\n\
         tomorrow docs/guide/capabilities.md\n\
         2026-13-01 docs/guide/agent-guide.md\n\
         2026-02-30 docs/adr/0001-a.md\n\
         2026-+1-09 docs/adr/0002-b.md\n\
         2026-10-31 site/agent-setup/prompt.md\n",
    ));
    repo.push((
        "site/agent-setup/prompt.md",
        "# Set up Oska\n\nInstall it.\n",
    ));
    let site = build_ok(&repo);
    let sitemap = &site["sitemap.xml"];
    assert!(
        sitemap
            .contains("<url><loc>https://oskalang.dev/</loc><lastmod>2026-11-03</lastmod></url>\n"),
        "{sitemap}"
    );
    assert!(
        sitemap
            .contains("<loc>https://oskalang.dev/glossary.html</loc><lastmod>2026-01-09</lastmod>"),
        "{sitemap}"
    );
    // A malformed date is skipped, and a page made from no one file, such
    // as the reference page, has no date.
    assert!(
        sitemap.contains(
            "<loc>https://oskalang.dev/agent-setup.html</loc><lastmod>2026-10-31</lastmod>"
        ),
        "{sitemap}"
    );
    assert_eq!(sitemap.matches("<lastmod>").count(), 3, "{sitemap}");
    assert!(sitemap.contains("<url><loc>https://oskalang.dev/reference.html</loc></url>\n"));
}

#[test]
fn without_lastmod_txt_the_sitemap_has_no_dates() {
    let site = build_ok(&agent_repo());
    assert!(
        !site["sitemap.xml"].contains("<lastmod>"),
        "{}",
        site["sitemap.xml"]
    );
}

/// The dates are git's: the workflow fetches the whole history, so a file's
/// last commit is its own, and writes `lastmod.txt` before the build.
#[test]
fn the_pages_workflow_writes_lastmod_txt_from_git_before_the_build() {
    let write = "> site/lastmod.txt";
    let run = "./target/release/oskac run site/build.osk";
    let at_write = PAGES_WORKFLOW
        .find(write)
        .unwrap_or_else(|| panic!("pages.yml does not write site/lastmod.txt"));
    let at_run = PAGES_WORKFLOW.find(run).expect("pages.yml builds the site");
    assert!(at_write < at_run, "lastmod.txt is written after the build");
    assert!(
        PAGES_WORKFLOW.contains("fetch-depth: 0"),
        "a shallow clone has no dates"
    );
    assert!(PAGES_WORKFLOW.contains("git log -1 --format=%cs"));
}
