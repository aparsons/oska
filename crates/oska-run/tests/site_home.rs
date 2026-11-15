//! The home page and the navigation (#188).
//!
//! The home page is its own page, `docs/index.md`, written for people and
//! agents who want to use Oska. The README stays the repository's front
//! page, for contributors, and the site does not read it. The navigation
//! leads with tasks, in a fixed order, and a Reference page holds the
//! internals, the glossary, the decisions and the research.
//!
//! The generator runs through the interpreter on a [`FakeHost`], and
//! natively on aarch64 Linux (`common`).

mod common;

use std::collections::BTreeMap;

use common::site::{self, between, MINIMAL_REPO};

/// The site the generator writes from [`MINIMAL_REPO`] and `inputs`,
/// asserting that it succeeded.
fn site_of(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    site::site_of(MINIMAL_REPO, inputs)
}

/// Every `href` in `html`, in order.
fn hrefs(html: &str) -> Vec<&str> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// The links of a page's navigation, as `(href, label)`, in order.
fn navigation(page: &str) -> Vec<(String, String)> {
    let nav = between(page, "<nav aria-label=\"Site\">", "</nav>");
    nav.split("<a href=\"")
        .skip(1)
        .map(|rest| {
            let href = rest.split('"').next().unwrap_or("").to_string();
            let label = between(rest, ">", "</a>").to_string();
            (href, label)
        })
        .collect()
}

/// A repository with every page the navigation can link.
const FULL_REPO: &[(&str, &str)] = &[
    ("docs/index.md", "# Oska\n\nA language.\n"),
    ("SECURITY.md", "# Security policy\n\nReport privately.\n"),
    ("docs/ast.md", "# AST\n\nThe tree.\n"),
    ("docs/checker.md", "# Checker\n\nThe checks.\n"),
    ("docs/runtime.md", "# Runtime\n\nThe interpreter.\n"),
    ("docs/guide/index.md", "# Guides\n\nStart here.\n"),
    ("docs/guide/install.md", "# Install\n\nIt installs.\n"),
    ("docs/guide/quickstart.md", "# Quickstart\n\nA program.\n"),
    (
        "docs/guide/language-tour.md",
        "# Language tour\n\nSyntax.\n",
    ),
    ("docs/guide/testing.md", "# Testing\n\nTests.\n"),
    ("docs/adr/0001-a.md", "# A decision\n\nWe chose A.\n"),
    ("docs/research/2026-11-x.md", "# A study\n\nWe looked.\n"),
    (
        "site/agent-setup/prompt.md",
        "# Set up Oska\n\nInstall it.\n",
    ),
    (
        "site/authority.txt",
        "fn main: FsRead (read), FsWrite (write)\n",
    ),
];

// ---- The home page --------------------------------------------------------------------

#[test]
fn the_home_page_is_built_from_docs_index_md_and_not_from_the_readme() {
    let site = site_of(&[
        ("README.md", "# Oska\n\nFor contributors: run cargo.\n"),
        (
            "docs/index.md",
            "# Oska\n\nA language for *people* and agents.\n",
        ),
    ]);
    let home = &site["index.html"];
    assert!(
        home.contains("<p>A language for <em>people</em> and agents.</p>"),
        "{home}"
    );
    assert!(!home.contains("contributors"), "{home}");
    assert_eq!(
        site["index.md"],
        "# Oska\n\nA language for *people* and agents.\n"
    );
    assert!(
        !site.values().any(|text| text.contains("For contributors")),
        "the README is published somewhere"
    );
}

#[test]
fn without_a_home_page_the_build_fails_and_names_docs_index_md() {
    let files: Vec<(&str, &str)> = MINIMAL_REPO
        .iter()
        .copied()
        .filter(|(path, _)| *path != "docs/index.md")
        .chain([("README.md", "# Oska\n")])
        .collect();
    let (outcome, _) = site::generate(&files, &[]);
    assert_eq!(outcome.exit_code, 1, "the build should fail: {outcome:#?}");
    assert!(
        outcome.stderr.contains("docs/index.md"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn a_link_to_the_readme_goes_to_it_on_github() {
    let site = site_of(&[(
        "docs/design.md",
        "# Design\n\nSee the [readme](../README.md).\n",
    )]);
    let readme = "https://github.com/aparsons/oska/blob/main/README.md";
    assert!(
        site["design.html"].contains(&format!("<a href=\"{readme}\">readme</a>")),
        "{}",
        site["design.html"]
    );
    assert!(site["design.md"].contains(&format!("[readme]({readme})")));
}

#[test]
fn the_home_page_shows_the_agent_prompt_after_its_install_section() {
    let site = site_of(&[
        (
            "docs/index.md",
            "# Oska\n\nA language.\n\n## Install\n\nRun the installer.\n\n## A first program\n\nHello.\n",
        ),
        ("site/agent-setup/prompt.md", "# Set up Oska\n\nInstall it.\n"),
    ]);
    let home = &site["index.html"];
    let install = home
        .find("Run the installer.")
        .expect("the install section");
    let prompt = home
        .find("<section class=\"agent-setup\"")
        .unwrap_or_else(|| panic!("no prompt on the home page:\n{home}"));
    let program = home
        .find("<h2 id=\"a-first-program\">")
        .expect("the next section");
    assert!(install < prompt && prompt < program, "{home}");
    let copy = &site["index.md"];
    let pointer = copy
        .find("https://oskalang.dev/agent-setup.md")
        .expect("the copy points at the prompt");
    assert!(copy.find("Run the installer.").expect("install") < pointer);
    assert!(pointer < copy.find("## A first program").expect("program"));
}

// ---- The navigation -------------------------------------------------------------------

#[test]
fn the_navigation_leads_with_tasks_in_a_fixed_order() {
    let site = site_of(FULL_REPO);
    let expected = [
        ("/", "Oska"),
        ("/guide/install.html", "Install"),
        ("/guide/quickstart.html", "Quickstart"),
        ("/guide/language-tour.html", "Tour"),
        ("/guide/", "Guides"),
        ("/agent-setup.html", "Agent setup"),
        ("/codes/", "Codes"),
        ("/security.html", "Security"),
        ("/reference.html", "Reference"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(href, label)| (href.to_string(), label.to_string()))
        .collect();
    for page in [
        "index.html",
        "design.html",
        "guide/testing.html",
        "reference.html",
    ] {
        assert_eq!(navigation(&site[page]), expected, "{page}");
    }
}

#[test]
fn the_navigation_marks_the_page_it_is_on() {
    let site = site_of(FULL_REPO);
    let nav = between(&site["guide/quickstart.html"], "<nav", "</nav>");
    assert!(
        nav.contains("<a href=\"/guide/quickstart.html\" aria-current=\"page\">Quickstart</a>"),
        "{nav}"
    );
    assert_eq!(nav.matches("aria-current").count(), 1, "{nav}");
}

#[test]
fn a_navigation_entry_whose_page_is_not_published_is_left_out() {
    let site = site_of(&[]);
    let nav: Vec<String> = navigation(&site["index.html"])
        .into_iter()
        .map(|(href, _)| href)
        .collect();
    assert_eq!(nav, ["/", "/reference.html"]);
}

// ---- The Reference page ----------------------------------------------------------------

#[test]
fn the_reference_page_holds_the_internals_glossary_decisions_and_research() {
    let site = site_of(FULL_REPO);
    let page = between(
        &site["reference.html"],
        "<main id=\"main-content\">",
        "</main>",
    );
    assert!(page.contains("<h1>Reference</h1>"), "{page}");
    let links = hrefs(page);
    for href in [
        "/ast.html",
        "/checker.html",
        "/design.html",
        "/runtime.html",
        "/authority.html",
        "/glossary.html",
        "/adr/",
        "/research/",
    ] {
        assert!(links.contains(&href), "{href} is not in {links:?}");
    }
    // The home page, the security policy and the guides are in the
    // navigation, not here.
    for href in ["/", "/index.html", "/security.html", "/guide/"] {
        assert!(!links.contains(&href), "{href} is in {links:?}");
    }
    let copy = &site["reference.md"];
    assert!(copy.starts_with("# Reference\n"), "{copy}");
    for link in [
        "(/ast.md)",
        "(/glossary.md)",
        "(/adr/index.md)",
        "(/research/index.md)",
    ] {
        assert!(copy.contains(link), "{link} is not in {copy}");
    }
}

#[test]
fn the_reference_page_is_a_page_like_any_other() {
    let site = site_of(FULL_REPO);
    assert!(site["sitemap.xml"].contains("<loc>https://oskalang.dev/reference.html</loc>"));
    assert!(site["llms-full.txt"].contains("Source: https://oskalang.dev/reference.html\n"));
    assert!(site["search.json"].contains("\"url\": \"/reference.html\""));
}

#[test]
fn the_footer_links_the_research() {
    let site = site_of(FULL_REPO);
    for page in ["index.html", "design.html", "reference.html"] {
        let footer = between(&site[page], "<footer>", "</footer>");
        assert!(hrefs(footer).contains(&"/research/"), "{page}: {footer}");
    }
    let bare = site_of(&[]);
    let footer = between(&bare["index.html"], "<footer>", "</footer>");
    assert!(!footer.contains("/research/"), "{footer}");
}

// ---- The real site ---------------------------------------------------------------------

#[test]
fn the_real_home_page_leads_with_install_the_prompt_and_the_quickstart() {
    let site = site::real_site();
    let home = between(&site["index.html"], "<main id=\"main-content\">", "</main>");
    // In the order a newcomer needs them.
    let at = |needle: &str| {
        home.find(needle)
            .unwrap_or_else(|| panic!("the home page has no {needle:?}:\n{home}"))
    };
    let order = [
        at("curl -fsSL https://oskalang.dev/install.sh | sh"),
        at("irm https://oskalang.dev/install.ps1 | iex"),
        at("<section class=\"agent-setup\""),
        at("<code>oskac check"),
        at("href=\"/guide/quickstart.html\""),
        at("href=\"/guide/what-works-today.html\""),
        at("href=\"/security.html\""),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}\n{home}");
    assert!(home.contains("href=\"/guide/install.html\""), "{home}");
    // No link from the home page to itself, and no contributor material.
    for href in hrefs(home) {
        assert!(
            ![
                "/",
                "/index.html",
                "https://oskalang.dev",
                "https://oskalang.dev/"
            ]
            .contains(&href),
            "the home page links to itself: {href}"
        );
    }
    assert!(!home.contains("cargo build"), "{home}");
    assert!(!home.contains("Repository layout"), "{home}");
}

#[test]
fn the_real_navigation_and_reference_page_reach_every_section() {
    let site = site::real_site();
    let nav: Vec<String> = navigation(&site["index.html"])
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    assert_eq!(
        nav,
        [
            "Oska",
            "Install",
            "Quickstart",
            "Tour",
            "Guides",
            "Agent setup",
            "Codes",
            "Security",
            "Reference"
        ]
    );
    let reference = hrefs(between(
        &site["reference.html"],
        "<main id=\"main-content\">",
        "</main>",
    ))
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    for href in [
        "/ast.html",
        "/checker.html",
        "/design.html",
        "/runtime.html",
        "/glossary.html",
        "/adr/",
        "/research/",
    ] {
        assert!(reference.iter().any(|h| h == href), "{href}: {reference:?}");
    }
}

#[test]
fn what_works_today_is_a_guide_listed_in_llms_txt() {
    let site = site::real_site();
    assert!(site.contains_key("guide/what-works-today.html"));
    assert!(
        site["llms.txt"].contains("](https://oskalang.dev/guide/what-works-today.md)"),
        "{}",
        site["llms.txt"]
    );
}
