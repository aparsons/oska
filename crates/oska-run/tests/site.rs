//! The site generator, `site/build.osk`, run through the interpreter on a
//! [`FakeHost`].
//!
//! Each directory in `site/tests/` is a golden case: its `input/` files
//! are put in the fake filesystem as if they were the repository, the
//! generator runs with no `_site/`, and what it writes there must match
//! `expected/` file for file. To add a case, add a directory with its
//! `input/`; to regenerate `expected/` after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oska-run --test site` and review the
//! diff.
//!
//! Every page any test builds also goes through [`assert_inert`], which
//! reads it the way a browser would and fails on anything that could run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oska_run::{run, FakeHost, RunOutcome};

/// The generator's source.
const BUILD: &str = include_str!("../../../site/build.osk");

/// Where the generator writes, relative to the repository root.
const OUTPUT: &str = "_site";

/// The Content Security Policy every page carries: nothing runs, and
/// styles and fonts come only from the site and the pinned CDN.
const CSP: &str = "default-src 'none'; style-src 'self' https://cdn.jsdelivr.net; \
                   font-src https://cdn.jsdelivr.net; img-src 'self'; base-uri 'none'; \
                   form-action 'none'";

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/tests")
}

/// Every file under `dir`, by its path relative to `dir`, with `/`
/// between components.
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("should list {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(root, &path, out);
                continue;
            }
            let relative = path.strip_prefix(root).expect("under the root");
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("should read {}: {e}", path.display()));
            out.insert(name, text);
        }
    }
    let mut out = BTreeMap::new();
    if dir.is_dir() {
        walk(dir, dir, &mut out);
    }
    out
}

/// What a run of the generator did: how it ended, and the files it left
/// in `_site/`, by path relative to it.
struct Build {
    outcome: RunOutcome,
    site: BTreeMap<String, String>,
}

/// Run the generator on a repository holding `inputs`, paths relative to
/// its root, with no `_site/`.
fn build(inputs: &BTreeMap<String, String>) -> Build {
    let mut host = inputs
        .iter()
        .fold(FakeHost::new(), |host, (path, text)| host.with_file(path, text));
    let outcome = run(BUILD, &mut host, &["site/build.osk".to_string()]);
    Build {
        outcome,
        site: host.files_under(OUTPUT),
    }
}

/// The files every repository the generator reads must have, as small as
/// they can be. A test's own inputs replace them.
const MINIMAL_REPO: &[(&str, &str)] = &[
    ("README.md", "# Oska\n"),
    ("GLOSSARY.md", "# Glossary\n"),
    ("docs/design.md", "# Design\n"),
    ("site/style.css", "body {}\n"),
];

/// [`build`] on [`MINIMAL_REPO`] with `inputs` added, asserting that the
/// generator succeeded and that every page it wrote is inert.
fn build_ok(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let inputs = MINIMAL_REPO
        .iter()
        .chain(inputs)
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    assert_site_inert(&built.site);
    built.site
}

fn assert_succeeded(outcome: &RunOutcome) {
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
}

/// Assert that the generator, run on the case `name`'s `input/`, writes
/// exactly its `expected/`, and that every page is inert.
fn assert_golden(name: &str) {
    let case = cases_dir().join(name);
    let inputs = read_tree(&case.join("input"));
    assert!(!inputs.is_empty(), "{} has no input files", case.display());
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    assert_site_inert(&built.site);
    let expected_dir = case.join("expected");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let _ = std::fs::remove_dir_all(&expected_dir);
        for (path, text) in &built.site {
            let file = expected_dir.join(path);
            let parent = file.parent().expect("a file has a parent");
            std::fs::create_dir_all(parent).expect("should create the expected directory");
            std::fs::write(&file, text).expect("should write the expected file");
        }
    }
    assert_eq!(
        built.site,
        read_tree(&expected_dir),
        "`_site/` differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended",
        expected_dir.display()
    );
}

// ---- What a browser would make of a page -------------------------------------

/// Every element the site may emit, with the attributes it may carry.
/// Anything else in a page, a `<script>` or an `onerror=`, means text
/// got out of its escaping.
const ELEMENTS: &[(&str, &[&str])] = &[
    ("html", &["lang"]),
    ("head", &[]),
    ("meta", &["charset", "name", "content", "http-equiv"]),
    ("title", &[]),
    ("link", &["rel", "href", "integrity", "crossorigin"]),
    ("body", &[]),
    ("header", &[]),
    ("nav", &["aria-label"]),
    ("main", &[]),
    ("footer", &[]),
    ("a", &["href", "aria-current"]),
    ("h1", &["id"]),
    ("h2", &["id"]),
    ("h3", &["id"]),
    ("h4", &["id"]),
    ("h5", &["id"]),
    ("h6", &["id"]),
    ("p", &[]),
    ("ul", &[]),
    ("ol", &["start"]),
    ("li", &[]),
    ("pre", &[]),
    ("code", &[]),
    ("blockquote", &[]),
    ("table", &[]),
    ("thead", &[]),
    ("tbody", &[]),
    ("tr", &[]),
    ("th", &[]),
    ("td", &[]),
    ("strong", &[]),
    ("em", &[]),
    ("img", &["src", "alt"]),
];

/// `text` with its character references decoded, as a browser decodes an
/// attribute value: the named ones the escaper writes, and numeric ones.
fn decode_references(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `url`, an attribute value as the browser decodes it, can only
/// navigate or load: a relative URL, a fragment, or `https:`, `http:`, or
/// `mailto:`. Browsers drop tabs and line breaks anywhere in a URL and
/// ignore the case of its scheme, so they are dropped and ignored here.
fn is_inert_url(url: &str) -> bool {
    let url: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase();
    let scheme_end = url.find(':');
    let path_start = url.find(['/', '?', '#']);
    match (scheme_end, path_start) {
        (None, _) => true,
        (Some(colon), Some(slash)) if slash < colon => true,
        _ => ["https:", "http:", "mailto:"]
            .iter()
            .any(|ok| url.starts_with(ok)),
    }
}

/// Assert that `page`, an HTML page the generator wrote, holds nothing a
/// browser would run: only the elements and attributes in [`ELEMENTS`],
/// every attribute double-quoted, every URL inert, no comment, and the
/// site's CSP before anything it governs.
fn assert_inert(name: &str, page: &str) {
    let rest = page
        .strip_prefix("<!DOCTYPE html>\n")
        .unwrap_or_else(|| panic!("{name} should start with a doctype:\n{page}"));
    let mut rest = rest;
    let mut seen_csp = false;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest
            .find('>')
            .unwrap_or_else(|| panic!("{name}: a `<` that never closes:\n{page}"));
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        let tag = tag.strip_prefix('/').unwrap_or(tag);
        let element: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '!' || *c == '-')
            .collect();
        let allowed = ELEMENTS
            .iter()
            .find(|(e, _)| *e == element)
            .unwrap_or_else(|| panic!("{name}: element `<{tag}>` is not allowed:\n{page}"))
            .1;
        let mut attributes = tag[element.len()..].trim_start();
        while !attributes.is_empty() {
            let eq = attributes
                .find("=\"")
                .unwrap_or_else(|| panic!("{name}: `<{tag}>`: an unquoted attribute"));
            let attribute = &attributes[..eq];
            assert!(
                allowed.contains(&attribute),
                "{name}: `{attribute}` on `<{element}>` is not allowed: `<{tag}>`"
            );
            let value_and_rest = &attributes[eq + 2..];
            let end = value_and_rest
                .find('"')
                .unwrap_or_else(|| panic!("{name}: `<{tag}>`: an attribute that never ends"));
            let value = decode_references(&value_and_rest[..end]);
            if attribute == "href" || attribute == "src" {
                assert!(
                    is_inert_url(&value),
                    "{name}: `{attribute}=\"{value}\"` could run: `<{tag}>`"
                );
            }
            if attribute == "http-equiv" {
                assert_eq!(value, "Content-Security-Policy", "{name}");
                seen_csp = true;
            }
            if element == "link" || element == "img" {
                assert!(seen_csp, "{name}: `<{tag}>` comes before the CSP");
            }
            attributes = value_and_rest[end + 1..].trim_start();
        }
    }
    assert!(seen_csp, "{name} has no CSP");
}

fn assert_site_inert(site: &BTreeMap<String, String>) {
    for (name, text) in site {
        if name.ends_with(".html") {
            assert_inert(name, text);
        }
    }
}

// ---- The golden cases ----------------------------------------------------------

#[test]
fn every_golden_case_builds_its_expected_site() {
    let entries = std::fs::read_dir(cases_dir()).expect("site/tests should exist");
    let mut names: Vec<String> = entries
        .map(|e| e.expect("a directory entry").file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "site/tests has no cases");
    for name in names {
        assert_golden(&name);
    }
}

// ---- Hostile Markdown -------------------------------------------------------------

/// The hostile case's design notes, the page every hostile construct is
/// in.
fn hostile_page() -> String {
    let case = cases_dir().join("hostile/input");
    let inputs = read_tree(&case);
    let refs: Vec<(&str, &str)> = inputs
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    build_ok(&refs)["design.html"].clone()
}

#[test]
fn raw_html_in_markdown_comes_out_as_text() {
    let page = hostile_page();
    for raw in [
        "&lt;script&gt;alert(&quot;raw&quot;)&lt;/script&gt;",
        "&lt;img src=x onerror=alert(1)&gt;",
        "&lt;iframe src=&quot;javascript:alert(1)&quot;&gt;&lt;/iframe&gt;",
        "&lt;!-- a comment --&gt;",
        "&lt;style&gt;body{display:none}&lt;/style&gt;",
    ] {
        assert!(page.contains(raw), "`{raw}` should be in:\n{page}");
    }
}

#[test]
fn a_script_link_is_not_a_link() {
    let page = hostile_page();
    for text in [
        "js lower", "js upper", "js tab", "data", "vbscript", "js entity", "js in image",
    ] {
        assert!(
            page.contains(&format!("<li>{text}</li>")),
            "`{text}` should be bare text in:\n{page}"
        );
    }
}

#[test]
fn a_quote_in_a_url_never_ends_its_attribute() {
    let page = hostile_page();
    assert!(!page.contains("\" onmouseover"), "{page}");
    assert!(!page.contains("\"onmouseover"), "{page}");
    assert!(!page.contains("\" onerror"), "{page}");
}

#[test]
fn a_safe_link_keeps_its_url_escaped() {
    let page = hostile_page();
    for anchor in [
        "<a href=\"https://example.com/safe?a=1&amp;b=2\">ok</a>",
        "<a href=\"mailto:someone@example.com\">mail</a>",
        "<a href=\"/glossary.html#capability\">glossary</a>",
    ] {
        assert!(page.contains(anchor), "`{anchor}` should be in:\n{page}");
    }
}

#[test]
fn an_image_loads_only_from_the_site() {
    let page = hostile_page();
    assert!(
        page.contains("<img src=\"pic.png\" alt=\"x&quot; onerror=&quot;alert(1)\">"),
        "{page}"
    );
    assert!(!page.contains("evil.example"), "{page}");
}

#[test]
fn character_references_are_shown_not_decoded() {
    let page = hostile_page();
    assert!(
        page.contains("&amp;lt;script&amp;gt; &amp;#60;script&amp;#62; &amp;amp;"),
        "{page}"
    );
}

#[test]
fn a_hostile_file_name_is_escaped_where_it_is_linked() {
    let site = build_ok(&[(
        "docs/adr/x\" onmouseover=\"alert(1).md",
        "# A <b>title</b>\n",
    )]);
    let page = &site["adr/index.html"];
    assert!(
        page.contains("href=\"/adr/x&quot; onmouseover=&quot;alert(1).html\""),
        "{page}"
    );
    assert!(page.contains("A &lt;b&gt;title&lt;/b&gt;"), "{page}");
}

// ---- What every page carries -------------------------------------------------------

#[test]
fn every_page_has_the_csp_and_pins_its_fonts_with_integrity() {
    let site = build_ok(&[]);
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        assert!(
            page.contains(&format!(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"
            )),
            "{name}:\n{page}"
        );
        assert!(!page.contains("<script"), "{name} runs JavaScript");
        for link in page.match_indices("<link ").map(|(at, _)| &page[at..]) {
            let link = &link[..link.find('>').expect("a closed tag")];
            if link.contains("https://") {
                assert!(
                    link.contains("https://cdn.jsdelivr.net/npm/@fontsource/"),
                    "{link}"
                );
                assert!(link.contains("@5.3.0/"), "{link} is not pinned");
                assert!(link.contains(" integrity=\"sha384-"), "{link}");
                assert!(link.contains(" crossorigin=\"anonymous\""), "{link}");
            }
        }
        assert!(page.contains("<link rel=\"stylesheet\" href=\"/style.css\">"));
    }
}

#[test]
fn the_stylesheet_and_the_cname_are_written() {
    let site = build_ok(&[("site/style.css", "main { color: red }\n")]);
    assert_eq!(site["style.css"], "main { color: red }\n");
    assert_eq!(site["CNAME"], "oskalang.dev\n");
}

#[test]
fn the_readme_becomes_the_home_page_and_the_glossary_its_own() {
    let site = build_ok(&[
        ("README.md", "# Oska\n\nA *language*.\n"),
        ("GLOSSARY.md", "# Words\n\n**Capability**:\nA value.\n"),
    ]);
    assert!(site["index.html"].contains("<p>A <em>language</em>.</p>"));
    assert!(site["glossary.html"].contains("<p><strong>Capability</strong>:\nA value.</p>"));
}

#[test]
fn the_glossary_page_is_titled_glossary_whatever_its_heading() {
    let site = build_ok(&[("GLOSSARY.md", "# Oska\n\nTerms.\n")]);
    assert!(site["glossary.html"].contains("<title>Glossary · Oska</title>"));
    assert!(site["glossary.html"].contains("<h1 id=\"oska\">Oska</h1>"));
}

// ---- The Markdown subset -------------------------------------------------------------

/// The `<main>` of the page `docs/design.md` with `markdown` becomes.
fn rendered(markdown: &str) -> String {
    let site = build_ok(&[("docs/design.md", markdown)]);
    let page = &site["design.html"];
    let start = page.find("<main>\n").expect("a main") + "<main>\n".len();
    let end = page.find("</main>").expect("a main end");
    page[start..end].to_string()
}

#[test]
fn headings_get_github_anchors() {
    assert_eq!(
        rendered("## Untrusted data, and `sinks`!\n"),
        "<h2 id=\"untrusted-data-and-sinks\">Untrusted data, and <code>sinks</code>!</h2>\n"
    );
}

#[test]
fn lists_nest_by_indentation_and_keep_their_start() {
    assert_eq!(
        rendered("- a\n  - b\n- c\n\n3. three\n4. four\n"),
        "<ul>\n<li>a<ul>\n<li>b</li>\n</ul>\n</li>\n<li>c</li>\n</ul>\n\
         <ol start=\"3\">\n<li>three</li>\n<li>four</li>\n</ol>\n"
    );
}

#[test]
fn an_item_with_a_blank_line_in_it_wraps_its_paragraphs() {
    assert_eq!(
        rendered("- one\n\n  more\n"),
        "<ul>\n<li><p>one</p>\n<p>more</p>\n</li>\n</ul>\n"
    );
}

#[test]
fn a_fence_keeps_its_text_verbatim_and_escaped() {
    assert_eq!(
        rendered("```oska\nlet x = \"<a>\" // **not bold**\n```\n"),
        "<pre><code>let x = &quot;&lt;a&gt;&quot; // **not bold**\n</code></pre>\n"
    );
}

#[test]
fn a_table_keeps_a_pipe_in_code_or_escaped() {
    assert_eq!(
        rendered("| a | b |\n|---|:-:|\n| `x | y` | p \\| q |\n"),
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n\
         <tr><td><code>x | y</code></td><td>p | q</td></tr>\n</tbody>\n</table>\n"
    );
}

#[test]
fn emphasis_needs_text_beside_it_and_never_splits_a_word() {
    assert_eq!(
        rendered("**b** *e* _u_ snake_case_name 2 * 3 * 4 \\*not\\*\n"),
        "<p><strong>b</strong> <em>e</em> <em>u</em> snake_case_name 2 * 3 * 4 *not*</p>\n"
    );
}

#[test]
fn relative_links_go_to_the_pages_published_from_their_targets() {
    let site = build_ok(&[
        ("docs/adr/0001-a.md", "# A\n\n[design](../design.md#x) [b](0002-b.md) [up](../../README.md)\n"),
        ("docs/adr/0002-b.md", "# B\n"),
        ("docs/design.md", "# D\n\n[adr](adr/) [out](../../etc/passwd) [src](../crates/x.rs)\n"),
    ]);
    assert!(site["adr/0001-a.html"].contains(
        "<a href=\"/design.html#x\">design</a> <a href=\"/adr/0002-b.html\">b</a> <a href=\"/\">up</a>"
    ));
    assert!(site["design.html"].contains("<a href=\"/adr/\">adr</a> out src</p>"));
}

#[test]
fn docs_outside_the_published_set_are_left_out() {
    let site = build_ok(&[
        ("docs/agents/issue-tracker.md", "# Internal\n"),
        ("docs/notes.txt", "not markdown\n"),
        ("docs/.hidden.md", "# Hidden\n"),
    ]);
    assert!(!site.keys().any(|k| k.contains("agents")), "{:?}", site.keys());
    assert!(!site.keys().any(|k| k.contains("notes")), "{:?}", site.keys());
    assert!(!site.keys().any(|k| k.contains("hidden")), "{:?}", site.keys());
}

#[test]
fn a_rebuild_into_an_existing_site_succeeds() {
    let mut inputs: BTreeMap<String, String> = MINIMAL_REPO
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect();
    inputs.insert("docs/adr/0001-a.md".to_string(), "# A\n".to_string());
    inputs.insert("_site/adr/stale.html".to_string(), "old".to_string());
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    assert!(built.site.contains_key("adr/0001-a.html"));
}

#[test]
fn the_generator_reaches_only_the_filesystem_read_only_and_write_only() {
    let report = oska_check::check_module(BUILD);
    assert_eq!(report.diagnostics, Vec::new());
    let main = report
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("the generator has a `main`");
    assert_eq!(main.authority, ["FsRead", "FsWrite"]);
    for function in &report.functions {
        for capability in &function.authority {
            assert!(
                capability == "FsRead" || capability == "FsWrite",
                "`{}` reaches `{capability}`",
                function.name
            );
        }
    }
}

#[test]
fn main_narrows_reading_to_docs_and_the_files_it_names() {
    for narrowing in [
        "read.restrict_to(\"docs\")",
        "read.restrict_to(\"README.md\")",
        "read.restrict_to(\"GLOSSARY.md\")",
        "read.restrict_to(\"site/style.css\")",
        "write.restrict_to(\"_site\")",
    ] {
        assert!(BUILD.contains(narrowing), "`main` should narrow with {narrowing}");
    }
    assert!(!BUILD.contains("restrict_to(\".\")"));
}
