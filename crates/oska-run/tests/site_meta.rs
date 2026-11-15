//! Site head and polish (#190): the meta and Open Graph tags every page
//! carries, the favicon, the skip link, the not-found page, the
//! stylesheet's contrast and focus rules, and what the Pages smoke check
//! expects of them.
//!
//! The generator runs through the interpreter on a [`FakeHost`], and
//! natively on aarch64 and x86_64 Linux (`common`). Every page a test builds goes
//! through the same inertness check as in `site.rs`.

mod common;

use std::collections::BTreeMap;

use common::inert::{tags_of, url_of};
use common::site::{agent_repo, build_ok, CSP, FAVICON, PAGES_WORKFLOW};

/// The `content` of the `<meta>` in `page` whose `attribute` (`name` or
/// `property`) is `key`.
fn meta(page: &str, attribute: &str, key: &str) -> Option<String> {
    tags_of(page)
        .into_iter()
        .filter(|tag| tag.element == "meta" && tag.attribute(attribute) == Some(key))
        .find_map(|tag| tag.attribute("content").map(str::to_string))
}

/// The pages of `site`: its HTML, but not the 404 page, which is served at
/// any path and so is no page of the site's.
fn pages_of(site: &BTreeMap<String, String>) -> Vec<(&String, &String)> {
    site.iter()
        .filter(|(n, _)| n.ends_with(".html") && *n != "404.html")
        .collect()
}

/// What the home page's title says Oska is.
const HOME_TITLE: &str = "Oska: an AI-first, secure-by-default programming language";

#[test]
fn every_page_has_a_description_and_its_open_graph_title_description_url_and_type() {
    let site = build_ok(&agent_repo());
    let design = &site["design.html"];
    let described = |page: &str, key: &str| meta(page, "name", key);
    let og = |page: &str, key: &str| meta(page, "property", key);
    assert_eq!(
        described(design, "description").as_deref(),
        Some("How Oska works, and why.")
    );
    assert_eq!(og(design, "og:title").as_deref(), Some("Design · Oska"));
    assert_eq!(
        og(design, "og:description").as_deref(),
        Some("How Oska works, and why.")
    );
    assert_eq!(
        og(design, "og:url").as_deref(),
        Some("https://oskalang.dev/design.html")
    );
    assert_eq!(og(design, "og:type").as_deref(), Some("article"));
    let home = &site["index.html"];
    assert_eq!(og(home, "og:url").as_deref(), Some("https://oskalang.dev/"));
    assert_eq!(og(home, "og:type").as_deref(), Some("website"));
    for (name, page) in pages_of(&site) {
        for key in ["og:title", "og:description", "og:url", "og:type"] {
            assert!(og(page, key).is_some(), "{name} has no {key}");
        }
        assert!(described(page, "description").is_some(), "{name}");
        assert_eq!(
            og(page, "og:description"),
            described(page, "description"),
            "{name}"
        );
    }
}

#[test]
fn a_page_with_no_paragraph_still_has_a_description() {
    let site = build_ok(&[("docs/design.md", "# Design\n\n- only a list\n")]);
    let description = meta(&site["design.html"], "name", "description").expect("a description");
    assert!(description.contains("Oska"), "{description}");
}

#[test]
fn hostile_text_in_a_title_or_summary_is_escaped_in_the_meta_tags() {
    let site = build_ok(&[(
        "docs/design.md",
        "# <script>alert(1)</script>\n\nA \"quoted\" & <b>bold</b> summary.\n",
    )]);
    let page = &site["design.html"];
    for tag in [
        "<meta name=\"description\" content=\"A &quot;quoted&quot; &amp; &lt;b&gt;bold&lt;/b&gt; summary.\">",
        "<meta property=\"og:description\" content=\"A &quot;quoted&quot; &amp; &lt;b&gt;bold&lt;/b&gt; summary.\">",
        "<meta property=\"og:title\" content=\"&lt;script&gt;alert(1)&lt;/script&gt; · Oska\">",
    ] {
        assert!(page.contains(tag), "`{tag}` should be in:\n{page}");
    }
    assert!(!page.contains("<script>alert"), "{page}");
}

#[test]
fn the_home_title_says_what_oska_is_whatever_the_home_page_is_titled() {
    for home in ["# Oska\n\nHello.\n", "# Welcome\n\nHello.\n"] {
        let site = build_ok(&[("docs/index.md", home)]);
        let home = &site["index.html"];
        assert!(
            home.contains(&format!("<title>{HOME_TITLE}</title>")),
            "{home}"
        );
        assert_eq!(
            meta(home, "property", "og:title").as_deref(),
            Some(HOME_TITLE)
        );
        // Another page's title is still its own.
        assert!(site["design.html"].contains("<title>Design · Oska</title>"));
    }
}

#[test]
fn every_page_links_its_favicon_and_the_site_serves_it_without_widening_the_csp() {
    let site = build_ok(&agent_repo());
    assert_eq!(site["favicon.svg"], FAVICON);
    for (name, page) in pages_of(&site) {
        assert!(
            page.contains("<link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\">\n"),
            "{name}:\n{page}"
        );
        // The favicon is the site's own image: `img-src 'self'` covers it,
        // and the policy is the one every page has always carried.
        assert!(
            page.contains(&format!(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"
            )),
            "{name}"
        );
    }
}

#[test]
fn every_page_has_a_skip_link_before_the_navigation_and_a_main_element_to_land_on() {
    let site = build_ok(&agent_repo());
    for (name, page) in pages_of(&site) {
        let skip = page
            .find("<a class=\"skip-link\" href=\"#main-content\">Skip to content</a>")
            .unwrap_or_else(|| panic!("{name} has no skip link:\n{page}"));
        assert!(skip < page.find("<nav").expect("a navigation"), "{name}");
        assert!(page[..skip].ends_with("<body>\n"), "{name}");
        assert_eq!(
            page.matches("<main id=\"main-content\">").count(),
            1,
            "{name}"
        );
        assert_eq!(page.matches("id=\"main-content\"").count(), 1, "{name}");
    }
}

#[test]
fn the_not_found_page_has_search_and_root_relative_links_to_install_guides_and_llms_txt() {
    let site = build_ok(&[
        ("docs/guide/install.md", "# Install\n\nGet it.\n"),
        ("docs/guide/quickstart.md", "# Quickstart\n\nGo.\n"),
    ]);
    let page = &site["404.html"];
    assert!(
        page.contains("<title>Page not found · Oska</title>"),
        "{page}"
    );
    assert!(
        page.contains("<h1 id=\"page-not-found\">Page not found</h1>"),
        "{page}"
    );
    for link in ["/guide/install.html", "/guide/", "/llms.txt"] {
        assert!(
            page.contains(&format!("<a href=\"{link}\">")),
            "no link to {link} in:\n{page}"
        );
    }
    // Search is the one every page has: the header's box, added by the
    // script, which the page loads, and the page says so.
    assert!(
        page.contains("<script src=\"/search.js\"></script>"),
        "{page}"
    );
    assert!(page.contains("search"), "{page}");
    // It is served at any path, so no link on it is relative, and it names
    // no canonical URL or Markdown copy of its own.
    for tag in tags_of(page) {
        if let Some(url) = url_of(&tag) {
            assert!(
                url.starts_with('/') || url.starts_with('#') || url.starts_with("https://"),
                "`{url}` on the 404 page is relative"
            );
        }
        assert!(
            !(tag.element == "link"
                && matches!(tag.attribute("rel"), Some("canonical" | "alternate"))),
            "the 404 page names itself"
        );
    }
    // And it is no page of the site's.
    assert!(!site.contains_key("404.md"));
    assert!(!site["sitemap.xml"].contains("404"));
    assert!(!site["llms.txt"].contains("404"));
    assert!(!site["llms-full.txt"].contains("404"));
    assert!(!site["search.json"].contains("404"));
}

#[test]
fn the_not_found_page_links_only_pages_the_site_has() {
    let site = build_ok(&[]);
    let page = &site["404.html"];
    assert!(page.contains("<a href=\"/llms.txt\">"), "{page}");
    for link in ["/guide/install.html", "/guide/"] {
        assert!(!page.contains(&format!("href=\"{link}\"")), "{page}");
    }
}

// ---- Contrast and focus (the stylesheet) ----------------------------------------------

const STYLE: &str = include_str!("../../../site/style.css");

/// The colour `#rrggbb` as sRGB channels.
fn rgb(hex: &str) -> [f64; 3] {
    let hex = hex.trim().trim_start_matches('#');
    let channel =
        |at: usize| f64::from(u8::from_str_radix(&hex[at..at + 2], 16).expect("hex")) / 255.0;
    [channel(0), channel(2), channel(4)]
}

/// The WCAG 2 contrast ratio of two colours.
fn contrast(a: &str, b: &str) -> f64 {
    let luminance = |hex: &str| {
        let [r, g, b] = rgb(hex).map(|c| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The values the stylesheet gives the custom property `name`: first the
/// light scheme's, then the dark scheme's.
fn property_values(name: &str) -> Vec<String> {
    STYLE
        .match_indices(&format!("  {name}:"))
        .map(|(at, _)| {
            let rest = &STYLE[at + name.len() + 3..];
            rest[..rest.find(';').expect("a declaration ends")]
                .trim()
                .to_string()
        })
        .collect()
}

/// The declarations of the first rule in the stylesheet that starts with
/// `selector`.
fn rule(selector: &str) -> &'static str {
    let start = STYLE
        .find(&format!("{selector} {{"))
        .unwrap_or_else(|| panic!("no rule for {selector}"));
    let rest = &STYLE[start..];
    &rest[..rest.find('}').expect("the rule ends")]
}

#[test]
fn the_search_box_border_has_three_to_one_contrast_in_both_colour_schemes() {
    let borders = property_values("--input-border");
    let backgrounds = property_values("--background");
    assert_eq!(borders.len(), 2, "a light and a dark --input-border");
    assert_eq!(backgrounds.len(), 2);
    for (border, background) in borders.iter().zip(&backgrounds) {
        let ratio = contrast(border, background);
        assert!(ratio >= 3.0, "{border} on {background} is {ratio:.2}:1");
    }
    let search = rule(".search input");
    assert!(
        search.contains("border: 1px solid var(--input-border)"),
        "{search}"
    );
}

#[test]
fn every_focusable_element_gets_a_visible_focus_style() {
    let focus = rule(":focus-visible");
    assert!(
        focus.contains("outline: 2px solid var(--accent)"),
        "{focus}"
    );
    let squeezed = STYLE.replace(' ', "");
    assert!(!squeezed.contains("outline:none"), "an outline is removed");
    assert!(!squeezed.contains("outline:0"), "an outline is removed");
}

#[test]
fn the_skip_link_is_off_screen_until_it_has_focus() {
    assert!(rule(".skip-link").contains("position: absolute"));
    assert!(
        STYLE.contains(".skip-link:focus"),
        "no .skip-link:focus rule"
    );
}

#[test]
fn the_smoke_check_expects_the_favicon_and_the_custom_404_page() {
    assert!(
        PAGES_WORKFLOW.contains("security.md favicon.svg; do"),
        "the smoke step does not fetch favicon.svg"
    );
    assert!(PAGES_WORKFLOW.contains("test \"$status\" = 404"));
    assert!(PAGES_WORKFLOW.contains("grep -q 'Page not found'"));
}
