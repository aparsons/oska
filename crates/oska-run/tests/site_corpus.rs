//! The agent corpus (#189): what an AI agent loads from oskalang.dev, in
//! tiers.
//!
//! `llms.txt` opens with an ordered "Start here" list, then the other
//! sections, then an Optional section for what an agent may skip: the
//! research, which is proposals, and the internals most programs do not
//! need. `llms-full.txt` is ordered so that an agent that truncates it
//! keeps the most useful text, and leaves out the per-code pages and the
//! research, which are in `codes.json` and `llms-research.txt`. Research
//! pages say they are proposals. A code's summary is a whole sentence, and
//! each entry of `codes.json` has a `url`.
//!
//! The generator runs through the interpreter on a [`FakeHost`], and
//! natively on aarch64 and x86_64 Linux (`common`).

mod common;

use std::collections::BTreeMap;

use common::site::{self, check_links, tracked_files};

/// The notice a research page carries.
const NOTICE: &str = "Proposals and background, not documented behaviour.";

/// A repository with every page the corpus orders, one paragraph each.
const REPO: &[(&str, &str)] = &[
    ("docs/index.md", "# Oska\n\nA language.\n"),
    ("SECURITY.md", "# Security policy\n\nReport privately.\n"),
    ("GLOSSARY.md", "# Glossary\n\nThe words.\n"),
    ("docs/design.md", "# Design\n\nHow it works.\n"),
    ("docs/ast.md", "# AST\n\nThe tree.\n"),
    (
        "docs/checker.md",
        "# Checker\n\nThe checks.\n\n| Code | Meaning | Fix |\n| --- | --- | --- |\n\
         | `E0103` | A bare `{ ... }` block used as a statement. More text. | None. |\n\
         | `E0104` | A second thing. | None. |\n",
    ),
    ("docs/runtime.md", "# Runtime\n\nThe interpreter.\n"),
    ("docs/guide/index.md", "# Guides\n\nStart here.\n"),
    ("docs/guide/agent-guide.md", "# Agent guide\n\nThe loop.\n"),
    (
        "docs/guide/capabilities.md",
        "# Capabilities\n\nAuthority.\n",
    ),
    ("docs/guide/install.md", "# Install\n\nIt installs.\n"),
    ("docs/guide/language-tour.md", "# Tour\n\nSyntax.\n"),
    ("docs/guide/quickstart.md", "# Quickstart\n\nA program.\n"),
    ("docs/guide/testing.md", "# Testing\n\nTests.\n"),
    ("docs/guide/packages.md", "# Packages\n\nModules.\n"),
    (
        "docs/guide/cli-reference.md",
        "# CLI reference\n\nThe flags.\n\n## Platform support\n\nWhere it runs.\n",
    ),
    (
        "docs/guide/untrusted-data.md",
        "# Untrusted data\n\nInjection.\n",
    ),
    (
        "docs/guide/what-works-today.md",
        "# What works today\n\nA table.\n",
    ),
    ("docs/guide/examples/hello.md", "# hello\n\nHello.\n"),
    ("docs/adr/0001-a.md", "# A decision\n\nWe chose A.\n"),
    (
        "docs/research/2026-11-x.md",
        "# A study\n\nWe looked at X.\n\nSee [the design](../design.md).\n",
    ),
    (
        "docs/research/2026-11-y.md",
        "# Another study\n\nWe looked at Y.\n",
    ),
    (
        "site/agent-setup/prompt.md",
        "# Set up Oska\n\nInstall it.\n",
    ),
    (
        "site/agent-setup/AGENTS.template.md",
        "# AGENTS\n\nThe loop.\n",
    ),
    (
        "site/authority.txt",
        "fn main: FsRead (read), FsWrite (write)\n",
    ),
    ("site/style.css", "body {}\n"),
    ("site/search.js", "// search\n"),
    (
        "site/favicon.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    ),
    (
        "Cargo.toml",
        "[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
    ),
];

/// The site built from [`REPO`] with `inputs` added or put in place of its
/// files.
fn site_of(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    site::site_of(REPO, inputs)
}

/// The site built from [`REPO`] without the files at `left_out`.
fn site_without(left_out: &[&str]) -> BTreeMap<String, String> {
    let files: Vec<(&str, &str)> = REPO
        .iter()
        .filter(|(path, _)| !left_out.contains(path))
        .copied()
        .collect();
    site::site_of(&files, &[])
}

/// The text of the `## heading` group of `llms`, up to the next group.
fn group<'a>(llms: &'a str, heading: &str) -> &'a str {
    let open = format!("\n## {heading}\n");
    let start = llms
        .find(&open)
        .unwrap_or_else(|| panic!("no group {heading} in:\n{llms}"))
        + open.len();
    let rest = &llms[start..];
    &rest[..rest.find("\n## ").unwrap_or(rest.len())]
}

/// The URL of every entry in `text`, in order, without the origin.
fn entry_urls(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with("- ["))
        .map(|l| {
            let rest = l.split("](https://oskalang.dev/").nth(1).expect("a URL");
            rest.split(')').next().expect("a close").to_string()
        })
        .collect()
}

/// The `Source:` URL of every page in `full`, in order, without the origin.
fn sources(full: &str) -> Vec<String> {
    full.lines()
        .filter_map(|l| l.strip_prefix("Source: https://oskalang.dev/"))
        .map(|path| match path {
            "" => "index.html".to_string(),
            dir if dir.ends_with('/') => format!("{dir}index.html"),
            page => page.to_string(),
        })
        .collect()
}

// ---- llms.txt ---------------------------------------------------------------------------

#[test]
fn llms_txt_starts_with_the_pages_an_agent_needs_first_in_order() {
    let site = site_of(&[]);
    let llms = &site["llms.txt"];
    assert_eq!(
        entry_urls(group(llms, "Start here")),
        [
            "agent-setup.md",
            "guide/quickstart.md",
            "guide/language-tour.md",
            "guide/capabilities.md",
            "guide/untrusted-data.md",
            "guide/agent-guide.md",
            "codes.json",
            "guide/what-works-today.md",
        ],
        "{llms}"
    );
    // The group is the first after the introduction.
    let first = llms.find("\n## ").expect("a group");
    assert!(llms[first..].starts_with("\n## Start here\n"), "{llms}");
}

#[test]
fn a_start_here_entry_has_a_summary_and_is_listed_once() {
    let site = site_of(&[]);
    let llms = &site["llms.txt"];
    assert!(
        llms.contains("- [Quickstart](https://oskalang.dev/guide/quickstart.md): A program.\n"),
        "{llms}"
    );
    for url in [
        "agent-setup.md",
        "guide/quickstart.md",
        "guide/what-works-today.md",
        "codes.json",
    ] {
        let listed = entry_urls(llms).iter().filter(|u| *u == url).count();
        assert_eq!(listed, 1, "{url} listed {listed} times in:\n{llms}");
    }
}

#[test]
fn start_here_leaves_out_a_page_the_site_does_not_have() {
    let site = site_without(&["docs/guide/untrusted-data.md", "docs/checker.md"]);
    let start = entry_urls(group(&site["llms.txt"], "Start here"));
    assert!(!start.iter().any(|u| u.contains("untrusted")), "{start:?}");
    assert!(!start.contains(&"codes.json".to_string()), "{start:?}");
    assert_eq!(start.len(), 6, "{start:?}");
}

#[test]
fn the_other_sections_follow_and_hold_what_start_here_does_not() {
    let site = site_of(&[]);
    let llms = &site["llms.txt"];
    let at = |heading: &str| {
        llms.find(&format!("\n## {heading}\n"))
            .unwrap_or_else(|| panic!("no {heading}:\n{llms}"))
    };
    assert!(at("Start here") < at("Set up an agent"));
    assert!(at("Set up an agent") < at("Guides"));
    assert!(at("Guides") < at("Docs"));
    assert!(at("Docs") < at("Decisions"));
    assert!(at("Decisions") < at("Machine-readable"));
    assert!(at("Machine-readable") < at("Optional"));
    // The prompt is in Start here, and its other files are in their group.
    assert_eq!(
        entry_urls(group(llms, "Set up an agent")),
        ["agent-setup.txt", "agent-setup/AGENTS.template.md"]
    );
    let guides = entry_urls(group(llms, "Guides"));
    assert!(
        guides.contains(&"guide/install.md".to_string()),
        "{guides:?}"
    );
    assert!(guides.contains(&"guide/examples/hello.md".to_string()));
    for listed in ["guide/quickstart.md", "guide/what-works-today.md"] {
        assert!(!guides.contains(&listed.to_string()), "{listed} twice");
    }
}

#[test]
fn optional_holds_research_labelled_as_proposals_and_the_rarely_needed_internals() {
    let site = site_of(&[]);
    let llms = &site["llms.txt"];
    let optional = group(llms, "Optional");
    assert!(
        llms.trim_end().ends_with(optional.trim_end()),
        "last: {llms}"
    );
    let urls = entry_urls(optional);
    for url in [
        "research/2026-11-x.md",
        "research/2026-11-y.md",
        "llms-research.txt",
        "ast.md",
        "runtime.md",
    ] {
        assert!(urls.contains(&url.to_string()), "{url}: {urls:?}");
    }
    for line in optional.lines().filter(|l| l.contains("/research/")) {
        assert!(line.contains("Proposal"), "not labelled: {line}");
    }
    assert!(
        optional.contains(
            "- [A study](https://oskalang.dev/research/2026-11-x.md): Proposal: We looked at X.\n"
        ),
        "{optional}"
    );
    // None of it is anywhere else.
    for url in ["ast.md", "runtime.md", "research/2026-11-x.md"] {
        assert_eq!(
            llms.matches(&format!("](https://oskalang.dev/{url})"))
                .count(),
            1,
            "{url}:\n{llms}"
        );
    }
    assert!(!group(llms, "Docs").contains("runtime"), "{llms}");
    assert!(!llms.contains("\n## Research\n"), "{llms}");
}

#[test]
fn without_research_there_is_no_research_file_or_entry() {
    let site = site_without(&["docs/research/2026-11-x.md", "docs/research/2026-11-y.md"]);
    assert!(!site.contains_key("llms-research.txt"));
    assert!(
        !site["llms.txt"].contains("llms-research"),
        "{}",
        site["llms.txt"]
    );
    assert!(!site["llms-full.txt"].contains("llms-research"));
}

// ---- llms-full.txt -----------------------------------------------------------------------

#[test]
fn llms_full_txt_is_ordered_so_that_truncation_loses_the_least() {
    let site = site_of(&[]);
    let order = sources(&site["llms-full.txt"]);
    let at = |page: &str| {
        order
            .iter()
            .position(|p| p == page)
            .unwrap_or_else(|| panic!("no {page} in {order:?}"))
    };
    assert_eq!(order[0], "agent-setup.html", "{order:?}");
    let guides = [
        "guide/install.html",
        "guide/quickstart.html",
        "guide/examples/hello.html",
    ];
    let last_guide = guides.iter().map(|g| at(g)).max().expect("guides");
    for guide in guides {
        assert!(at("agent-setup.html") < at(guide), "{guide}");
    }
    assert!(last_guide < at("guide/what-works-today.html"), "{order:?}");
    assert!(at("guide/what-works-today.html") < at("glossary.html"));
    assert!(at("glossary.html") < at("design.html"));
    assert!(at("design.html") < at("checker.html"));
    assert!(at("checker.html") < at("adr/0001-a.html"));
}

#[test]
fn llms_full_txt_omits_the_code_pages_and_the_research_and_says_so() {
    let site = site_of(&[]);
    let full = &site["llms-full.txt"];
    for page in sources(full) {
        assert!(
            !page.starts_with("codes/") && !page.starts_with("research/"),
            "{page} is in llms-full.txt"
        );
    }
    assert!(!full.contains(NOTICE), "{full}");
    // The pages are still published, and the file says where they are.
    assert!(site.contains_key("codes/E0103.html"));
    assert!(site.contains_key("research/2026-11-x.html"));
    let head = &full[..full.find("\n---\n").expect("a first page")];
    assert!(
        head.contains("[codes.json](https://oskalang.dev/codes.json)"),
        "{head}"
    );
    assert!(
        head.contains("[llms-research.txt](https://oskalang.dev/llms-research.txt)"),
        "{head}"
    );
    assert!(head.contains("per-code pages"), "{head}");
    assert!(head.contains("research"), "{head}");
}

#[test]
fn llms_full_txt_holds_every_other_page_once() {
    let site = site_of(&[]);
    let order = sources(&site["llms-full.txt"]);
    let expected: Vec<String> = site
        .keys()
        .filter(|k| k.ends_with(".html") && *k != "404.html")
        .filter(|k| !k.starts_with("codes/") && !k.starts_with("research/"))
        .cloned()
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, expected, "{order:?}");
    // Internals and the rest come after the decisions.
    let adr = order
        .iter()
        .position(|p| p == "adr/0001-a.html")
        .expect("adr");
    for page in ["runtime.html", "ast.html", "index.html", "security.html"] {
        let at = order.iter().position(|p| p == page).expect(page);
        assert!(adr < at, "{page} before the decisions: {order:?}");
    }
}

#[test]
fn without_codes_or_research_llms_full_txt_has_nothing_to_say_about_them() {
    let site = site_without(&[
        "docs/checker.md",
        "docs/research/2026-11-x.md",
        "docs/research/2026-11-y.md",
    ]);
    let full = &site["llms-full.txt"];
    assert!(!full.contains("codes.json"), "{full}");
    assert!(!full.contains("llms-research"), "{full}");
}

// ---- llms-research.txt -------------------------------------------------------------------

#[test]
fn llms_research_txt_holds_the_research_and_says_it_is_proposals() {
    let site = site_of(&[]);
    let research = &site["llms-research.txt"];
    assert!(
        research.starts_with("# Oska: research (proposals)\n"),
        "{research}"
    );
    let head = &research[..research.find("\n---\n").expect("a first page")];
    assert!(head.contains(NOTICE), "{head}");
    assert!(head.contains("1.2.3"), "{head}");
    assert_eq!(
        sources(research),
        ["research/2026-11-x.html", "research/2026-11-y.html"]
    );
    for name in ["research/2026-11-x", "research/2026-11-y"] {
        let copy = &site[&format!("{name}.md")];
        let section = format!("\n---\n\nSource: https://oskalang.dev/{name}.html\n\n{copy}");
        assert!(research.contains(&section), "{name}:\n{research}");
    }
}

// ---- Research pages ----------------------------------------------------------------------

#[test]
fn a_research_page_says_in_html_and_markdown_that_it_holds_proposals() {
    let site = site_of(&[]);
    let page = &site["research/2026-11-x.html"];
    let main = &page[page.find("<main").expect("main")..];
    assert!(
        main.contains(&format!("<strong>{NOTICE}</strong>")),
        "{main}"
    );
    assert!(
        main.find(NOTICE).expect("notice") < main.find("<h1").expect("title"),
        "the notice comes first"
    );
    let copy = &site["research/2026-11-x.md"];
    assert!(copy.starts_with(&format!("> **{NOTICE}**")), "{copy}");
    assert!(copy.contains("# A study\n\nWe looked at X.\n"), "{copy}");
    // Its own links still resolve, and so do the notice's.
    assert!(copy.contains("[the design](/design.md)"), "{copy}");
    assert!(copy.contains("(/guide/what-works-today.md)"), "{copy}");
    assert!(
        main.contains("href=\"/guide/what-works-today.html\""),
        "{main}"
    );
}

#[test]
fn the_notice_does_not_change_the_title_or_the_summary() {
    let site = site_of(&[]);
    assert!(site["research/2026-11-x.html"].contains("<title>A study"));
    let llms = &site["llms.txt"];
    assert!(llms.contains(
        "[A study](https://oskalang.dev/research/2026-11-x.md): Proposal: We looked at X.\n"
    ));
    let research = &site["research/index.md"];
    assert!(
        research.contains("[A study](/research/2026-11-x.md)"),
        "{research}"
    );
    assert!(
        research.starts_with(&format!("> **{NOTICE}**")),
        "{research}"
    );
}

#[test]
fn other_pages_carry_no_notice() {
    let site = site_of(&[]);
    for page in [
        "design.md",
        "adr/0001-a.md",
        "guide/quickstart.md",
        "design.html",
        "adr/0001-a.html",
    ] {
        assert!(!site[page].contains(NOTICE), "{page}");
    }
}

#[test]
fn a_research_page_without_a_guide_has_a_notice_with_no_dead_link() {
    let site = site_without(&["docs/guide/what-works-today.md"]);
    let copy = &site["research/2026-11-x.md"];
    assert!(copy.starts_with(&format!("> **{NOTICE}**")), "{copy}");
    assert!(!copy.contains("what-works-today"), "{copy}");
}

// ---- The code catalog --------------------------------------------------------------------

#[test]
fn a_code_summary_is_a_whole_sentence_whatever_its_code_spans_hold() {
    let site = site_of(&[]);
    let index = &site["codes/index.md"];
    assert!(
        index.contains(
            "- [E0103](https://oskalang.dev/codes/E0103.md): A bare `{ ... }` block used as a statement\n"
        ),
        "{index}"
    );
    let page = &site["codes/index.html"];
    assert!(
        page.contains("<code>{ ... }</code> block used as a statement</li>"),
        "{page}"
    );
}

#[test]
fn a_sentence_ends_at_a_full_stop_outside_code_not_inside_it() {
    let checker = "# Checker\n\n| Code | Meaning | Fix |\n| --- | --- | --- |\n\
        | `E0100` | A `.` or `a. b` in code. Then more. | None. |\n\
        | `E0101` | Use ``a ` b. c`` here. Then more. | None. |\n\
        | `E0102` | No full stop | None. |\n\
        | `E0105` | Ends at the end of the cell. | None. |\n";
    let site = site_of(&[("docs/checker.md", checker)]);
    let index = &site["codes/index.md"];
    for line in [
        "[E0100](https://oskalang.dev/codes/E0100.md): A `.` or `a. b` in code\n",
        "[E0101](https://oskalang.dev/codes/E0101.md): Use ``a ` b. c`` here\n",
        "[E0102](https://oskalang.dev/codes/E0102.md): No full stop\n",
        "[E0105](https://oskalang.dev/codes/E0105.md): Ends at the end of the cell\n",
    ] {
        assert!(index.contains(line), "{line}\n{index}");
    }
}

#[test]
fn each_code_in_codes_json_has_its_url() {
    let site = site_of(&[]);
    let codes = &site["codes.json"];
    assert!(
        codes.contains("\"fix\":null,\"url\":\"https://oskalang.dev/codes/E0103.html\"}"),
        "{codes}"
    );
    assert_eq!(codes.matches("\"url\":").count(), 2, "{codes}");
    assert!(codes.contains("\"url\":\"https://oskalang.dev/codes/E0104.html\"}"));
    // The URL is a page the site has.
    assert!(site.contains_key("codes/E0104.html"));
}

// ---- The link checker --------------------------------------------------------------------

#[test]
fn the_checker_passes_the_corpus_and_fails_on_a_dead_link_in_llms_research_txt() {
    let site = site_of(&[]);
    let outcome = check_links(&site, &[]);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    let mut broken = site.clone();
    let research = format!(
        "{}\n[gone](https://oskalang.dev/gone.md)\n",
        site["llms-research.txt"]
    );
    broken.insert("llms-research.txt".to_string(), research);
    let outcome = check_links(&broken, &[]);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains("llms-research.txt: https://oskalang.dev/gone.md: "),
        "{}",
        outcome.stderr
    );
}

// ---- The real docs -----------------------------------------------------------------------

#[test]
fn the_real_corpus_is_tiered_and_every_link_in_it_goes_somewhere() {
    let site = site::real_site();
    let llms = &site["llms.txt"];
    let start = entry_urls(group(llms, "Start here"));
    assert_eq!(start.len(), 8, "{start:?}");
    assert_eq!(start[0], "agent-setup.md");
    assert_eq!(start[start.len() - 1], "guide/what-works-today.md");
    for url in entry_urls(llms) {
        assert!(site.contains_key(&url), "llms.txt lists /{url}");
    }
    assert!(group(llms, "Optional").contains("research/"), "{llms}");
    assert!(site.contains_key("llms-research.txt"));
    let full = &site["llms-full.txt"];
    assert!(!full.contains("Source: https://oskalang.dev/research/"));
    assert!(!full.contains("Source: https://oskalang.dev/codes/"));
    // Every code's summary in the index is a whole sentence: no line cut
    // off in the middle of a code span.
    for line in site["codes/index.md"]
        .lines()
        .filter(|l| l.starts_with("- ["))
    {
        assert_eq!(line.matches('`').count() % 2, 0, "{line}");
    }
    // The site, as built, passes the link checker.
    let tracked = tracked_files();
    let tracked: Vec<&str> = tracked.iter().map(String::as_str).collect();
    let outcome = check_links(&site, &tracked);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
}
