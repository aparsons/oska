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

/// The Content Security Policy every page carries: nothing runs but the
/// site's own search script, which may read only the site's own files,
/// and styles and fonts come only from the site and the two font packages,
/// at their pinned versions, on the CDN.
const CSP: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
    style-src 'self' https://cdn.jsdelivr.net/npm/@fontsource/inter@5.3.0/ \
    https://cdn.jsdelivr.net/npm/@fontsource/jetbrains-mono@5.3.0/; \
    font-src https://cdn.jsdelivr.net/npm/@fontsource/inter@5.3.0/ \
    https://cdn.jsdelivr.net/npm/@fontsource/jetbrains-mono@5.3.0/; \
    img-src 'self'; base-uri 'none'; form-action 'none'";

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
    let mut host = inputs.iter().fold(FakeHost::new(), |host, (path, text)| {
        host.with_file(path, text)
    });
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
    ("site/search.js", "// search\n"),
    (
        "Cargo.toml",
        "[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
    ),
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
/// Anything else in a page, an `<iframe>` or an `onerror=`, means text
/// got out of its escaping.
const ELEMENTS: &[(&str, &[&str])] = &[
    ("html", &["lang"]),
    ("head", &[]),
    ("meta", &["charset", "name", "content", "http-equiv"]),
    ("title", &[]),
    ("link", &["rel", "href", "type", "integrity", "crossorigin"]),
    ("script", &["type", "src"]),
    ("aside", &["aria-label"]),
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
    ("pre", &["id"]),
    ("section", &["class", "aria-labelledby"]),
    ("button", &["type", "data-copy", "hidden"]),
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
        "js lower",
        "js upper",
        "js tab",
        "data",
        "vbscript",
        "js entity",
        "js in image",
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
        for link in page.match_indices("<link ").map(|(at, _)| &page[at..]) {
            let link = &link[..link.find('>').expect("a closed tag")];
            if link.contains("https://") && !link.contains("rel=\"canonical\"") {
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
        (
            "docs/adr/0001-a.md",
            "# A\n\n[design](../design.md#x) [b](0002-b.md) [up](../../README.md)\n",
        ),
        ("docs/adr/0002-b.md", "# B\n"),
        (
            "docs/design.md",
            "# D\n\n[adr](adr/) [out](../../etc/passwd) [src](../crates/x.rs)\n",
        ),
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
    assert!(
        !site.keys().any(|k| k.contains("agents")),
        "{:?}",
        site.keys()
    );
    assert!(
        !site.keys().any(|k| k.contains("notes")),
        "{:?}",
        site.keys()
    );
    assert!(
        !site.keys().any(|k| k.contains("hidden")),
        "{:?}",
        site.keys()
    );
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
        "read.restrict_to(\"site/search.js\")",
        "read.restrict_to(\"Cargo.toml\")",
        "write.restrict_to(\"_site\")",
    ] {
        assert!(
            BUILD.contains(narrowing),
            "`main` should narrow with {narrowing}"
        );
    }
    assert!(!BUILD.contains("restrict_to(\".\")"));
}

// ---- The error-code catalog -----------------------------------------------------------

/// The site built from the `codes` case's inputs: a small `checker.md`
/// and `runtime.md` with hostile text in them.
fn codes_site() -> BTreeMap<String, String> {
    let inputs = read_tree(&cases_dir().join("codes/input"));
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    assert_site_inert(&built.site);
    built.site
}

#[test]
fn codes_json_holds_each_code_with_its_examples_and_escapes_them() {
    let expected = r#"[
{"code":"E0200","message":"Unknown name \"x\" with a \\\\ in it, as in `Strng`. See [the tour](#tour).","machine_fix":true,"machine_fix_description":"Replace the name with the closest one.","example":"fn f(x: Strng) {}\n","fix":"fn f(x: String) {}\n"},
{"code":"E0201","message":"A trait <script>alert(1)</script> where a type is expected. Then more.","machine_fix":false,"machine_fix_description":null,"example":"fn show(x: Display) {\n    let note = \"<b>\\\"quoted\\\"</b>\"\n}\n\n","fix":null},
{"code":"E0140","message":"An or-pattern such as `A | B`.","machine_fix":false,"machine_fix_description":null,"example":null,"fix":null},
{"code":"P0004","message":"Division by zero, see [Tests](#tests). Spans the operation.","machine_fix":false,"machine_fix_description":null,"example":null,"fix":null}
]
"#;
    assert_eq!(codes_site()["codes.json"], expected);
}

#[test]
fn a_code_page_shows_its_meaning_example_and_fix_escaped() {
    let site = codes_site();
    let page = &site["codes/E0200.html"];
    for part in [
        "<title>E0200 · Oska</title>",
        "<h1 id=\"e0200\">E0200</h1>",
        "<strong>Machine-applicable fix:</strong> Replace the name with the closest one.",
        "<h2 id=\"example\">Example</h2>",
        "<pre><code>fn f(x: Strng) {}\n</code></pre>",
        "<h2 id=\"fixed\">Fixed</h2>",
        "<pre><code>fn f(x: String) {}\n</code></pre>",
        "<a href=\"/checker.html#e0200\">checker notes</a>",
        "<a href=\"/codes/\">All error codes</a>",
    ] {
        assert!(page.contains(part), "`{part}` should be in:\n{page}");
    }
    // A link in the meaning that pointed within the doc points there still.
    assert!(
        page.contains("<a href=\"/checker.html#tour\">the tour</a>"),
        "{page}"
    );
    let hostile = &site["codes/E0201.html"];
    assert!(
        hostile.contains("A trait &lt;script&gt;alert(1)&lt;/script&gt; where"),
        "{hostile}"
    );
    assert!(hostile.contains("let note = &quot;&lt;b&gt;"), "{hostile}");
    assert!(
        hostile.contains("<strong>Machine-applicable fix:</strong> none."),
        "{hostile}"
    );
}

#[test]
fn a_code_with_no_example_has_no_example_section_and_a_runtime_code_links_the_runtime_notes() {
    let site = codes_site();
    let page = &site["codes/P0004.html"];
    assert!(!page.contains("id=\"example\""), "{page}");
    assert!(!page.contains("id=\"fixed\""), "{page}");
    assert!(
        page.contains("<a href=\"/runtime.html#tests\">Tests</a>"),
        "{page}"
    );
    assert!(
        page.contains("<a href=\"/runtime.html\">runtime notes</a>"),
        "{page}"
    );
    // A code with an example has a heading of its own to link to.
    assert!(
        site["codes/E0200.html"].contains("<a href=\"/checker.html#e0200\">checker notes</a>"),
        "{}",
        site["codes/E0200.html"]
    );
}

#[test]
fn the_index_links_every_code_with_its_first_sentence_and_the_navigation_links_the_index() {
    let site = codes_site();
    let index = &site["codes/index.html"];
    for part in [
        "<li><a href=\"/codes/E0200.html\">E0200</a>: Unknown name &quot;x&quot; with a \\ in it, as in <code>Strng</code></li>",
        "<li><a href=\"/codes/E0201.html\">E0201</a>: A trait &lt;script&gt;alert(1)&lt;/script&gt; where a type is expected</li>",
        "<li><a href=\"/codes/P0004.html\">P0004</a>: Division by zero, see <a href=\"/runtime.html#tests\">Tests</a></li>",
        "<a href=\"/codes.json\">codes.json</a>",
    ] {
        assert!(index.contains(part), "`{part}` should be in:\n{index}");
    }
    assert!(
        index.contains("<li><a href=\"/codes/\" aria-current=\"page\">Codes</a></li>"),
        "{index}"
    );
    assert!(site["index.html"].contains("<li><a href=\"/codes/\">Codes</a></li>"));
}

#[test]
fn without_a_checker_doc_there_is_no_catalog() {
    let site = build_ok(&[]);
    assert!(
        !site.keys().any(|k| k.starts_with("codes")),
        "{:?}",
        site.keys()
    );
    assert!(!site["index.html"].contains("Codes"));
}

// ---- Markdown copies for agents ----------------------------------------------------

/// The repository the agent outputs are tested on: a home page, a glossary,
/// two docs, and a section of two.
fn agent_repo() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "README.md",
            "# Oska\n\nA language where authority is explicit.\n\nSee the [design](docs/design.md).\n",
        ),
        ("GLOSSARY.md", "# Words\n\n**Capability**: a value.\n"),
        (
            "docs/design.md",
            "# Design\n\nHow Oska works, and why.\n\n## Values\n\nText.\n\n\
             [adr](adr/) [b](adr/0002-b.md#x) [up](../README.md) [out](https://example.com/a?b=1) \
             [src](../crates/x.rs) [here](#values)\n\n```\n[a](adr/)\n```\n",
        ),
        (
            "docs/adr/0001-a.md",
            "# A decision\n\nWe chose A.\n\n[design](../design.md#values)\n",
        ),
        ("docs/adr/0002-b.md", "# Another\n\nWe chose B.\n"),
    ]
}

/// The path of the Markdown copy of the page at `html`.
fn twin(html: &str) -> String {
    format!("{}.md", html.strip_suffix(".html").expect("an html page"))
}

#[test]
fn every_page_has_a_markdown_copy() {
    let site = build_ok(&agent_repo());
    let pages: Vec<&String> = site.keys().filter(|k| k.ends_with(".html")).collect();
    assert!(pages.len() >= 6, "{pages:?}");
    for page in pages {
        assert!(site.contains_key(&twin(page)), "{page} has no copy");
    }
}

#[test]
fn a_doc_s_copy_is_its_source() {
    let site = build_ok(&agent_repo());
    assert!(
        site["adr/0002-b.md"] == "# Another\n\nWe chose B.\n",
        "{}",
        site["adr/0002-b.md"]
    );
    assert!(site["index.md"].starts_with("# Oska\n\nA language where"));
    assert!(site["glossary.md"].starts_with("# Words\n"));
}

#[test]
fn a_section_index_copy_lists_its_docs() {
    let site = build_ok(&agent_repo());
    assert_eq!(
        site["adr/index.md"],
        "# Decisions\n\n- [A decision](/adr/0001-a.md)\n- [Another](/adr/0002-b.md)\n"
    );
}

#[test]
fn links_in_a_copy_point_at_other_copies() {
    let site = build_ok(&agent_repo());
    assert!(
        site["design.md"].contains(
            "[adr](/adr/index.md) [b](/adr/0002-b.md#x) [up](/index.md) \
             [out](https://example.com/a?b=1) [src](../crates/x.rs) [here](#values)"
        ),
        "{}",
        site["design.md"]
    );
    assert!(site["index.md"].contains("[design](/design.md)"));
    assert!(site["adr/0001-a.md"].contains("[design](/design.md#values)"));
}

#[test]
fn a_link_inside_a_code_fence_is_left_alone() {
    let site = build_ok(&agent_repo());
    assert!(site["design.md"].ends_with("```\n[a](adr/)\n```\n"));
}

// ---- JSON, to check what the generator writes is well formed ---------------------

/// A JSON value. There is no JSON crate to lean on, so this reads just
/// enough of the grammar to tell well-formed text from malformed.
#[derive(Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or_else(|| panic!("no `{key}` in {self:?}")),
            other => panic!("{other:?} is not an object"),
        }
    }

    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("{other:?} is not a string"),
        }
    }

    fn array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("{other:?} is not an array"),
        }
    }
}

struct JsonReader<'a> {
    text: &'a [u8],
    at: usize,
}

impl JsonReader<'_> {
    fn skip_space(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", byte as char, self.at))
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("bad word at byte {}", self.at))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_space();
        match self.text.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.skip_space();
                if self.text.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.skip_space();
                    let key = self.string()?;
                    self.eat(b':')?;
                    members.push((key, self.value()?));
                    self.skip_space();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(format!("bad object at byte {}", self.at)),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.skip_space();
                if self.text.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_space();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(format!("bad array at byte {}", self.at)),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.at;
                while matches!(
                    self.text.get(self.at),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.at += 1;
                }
                Ok(Json::Number(
                    String::from_utf8_lossy(&self.text[start..self.at]).into_owned(),
                ))
            }
            _ => Err(format!("unexpected input at byte {}", self.at)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = *self
                .text
                .get(self.at)
                .ok_or_else(|| "a string that never ends".to_string())?;
            self.at += 1;
            match byte {
                b'"' => {
                    return String::from_utf8(out).map_err(|e| e.to_string());
                }
                0..=0x1f => return Err(format!("raw control byte {byte:#x} in a string")),
                b'\\' => {
                    let escape = *self
                        .text
                        .get(self.at)
                        .ok_or_else(|| "a dangling backslash".to_string())?;
                    self.at += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let decoded = self
                                .text
                                .get(self.at..self.at + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32)
                                .ok_or_else(|| format!("bad \\u escape at byte {}", self.at))?;
                            self.at += 4;
                            decoded
                        }
                        other => return Err(format!("bad escape `\\{}`", other as char)),
                    };
                    let mut buffer = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                other => out.push(other),
            }
        }
    }
}

/// `text` as JSON, or why it is not.
fn parse_json(text: &str) -> Result<Json, String> {
    let mut reader = JsonReader {
        text: text.as_bytes(),
        at: 0,
    };
    let value = reader.value()?;
    reader.skip_space();
    if reader.at == text.len() {
        Ok(value)
    } else {
        Err(format!("trailing text at byte {}", reader.at))
    }
}

fn json(text: &str) -> Json {
    parse_json(text).unwrap_or_else(|e| panic!("not JSON ({e}):\n{text}"))
}

#[test]
fn the_json_reader_tells_json_from_not() {
    assert!(parse_json("{\"a\": [1, true, null, \"x\\n\\u00e9\"]}").is_ok());
    for bad in ["{\"a\": }", "[1,]", "\"a\nb\"", "{} x", "\"\\q\"", "[1 2]"] {
        assert!(parse_json(bad).is_err(), "{bad}");
    }
}

// ---- robots.txt, the sitemap, and latest.json ----------------------------------

#[test]
fn robots_txt_welcomes_search_engines_and_ai_crawlers() {
    let site = build_ok(&agent_repo());
    assert_eq!(
        site["robots.txt"],
        "User-agent: *\n\
         User-agent: GPTBot\n\
         User-agent: OAI-SearchBot\n\
         User-agent: ChatGPT-User\n\
         User-agent: ClaudeBot\n\
         User-agent: Claude-SearchBot\n\
         User-agent: Claude-User\n\
         User-agent: anthropic-ai\n\
         User-agent: PerplexityBot\n\
         User-agent: Perplexity-User\n\
         User-agent: Google-Extended\n\
         User-agent: Applebot-Extended\n\
         User-agent: CCBot\n\
         User-agent: Amazonbot\n\
         User-agent: meta-externalagent\n\
         User-agent: cohere-ai\n\
         Allow: /\n\
         \n\
         Sitemap: https://oskalang.dev/sitemap.xml\n"
    );
}

/// The URL path a page at `html` is served at, which is its canonical
/// URL's path: `/` for `index.html`, `/adr/` for `adr/index.html`.
fn canonical_path(html: &str) -> String {
    match html.strip_suffix("index.html") {
        Some(dir) => format!("/{dir}"),
        None => format!("/{html}"),
    }
}

#[test]
fn the_sitemap_lists_every_page_by_its_canonical_url() {
    let site = build_ok(&agent_repo());
    let sitemap = &site["sitemap.xml"];
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n";
    assert!(sitemap.starts_with(head), "{sitemap}");
    assert!(sitemap.ends_with("</urlset>\n"), "{sitemap}");
    let mut listed: Vec<String> = sitemap
        .lines()
        .filter_map(|l| l.strip_prefix("<url><loc>")?.strip_suffix("</loc></url>"))
        .map(str::to_string)
        .collect();
    let mut expected: Vec<String> = site
        .keys()
        .filter(|k| k.ends_with(".html"))
        .map(|k| format!("https://oskalang.dev{}", canonical_path(k)))
        .collect();
    listed.sort();
    expected.sort();
    assert_eq!(listed, expected);
    for url in [
        "https://oskalang.dev/",
        "https://oskalang.dev/adr/",
        "https://oskalang.dev/design.html",
    ] {
        assert!(listed.iter().any(|l| l == url), "{url} in {listed:?}");
    }
}

#[test]
fn latest_json_names_the_version_and_the_installers() {
    let site = build_ok(&agent_repo());
    let latest = json(&site["latest.json"]);
    assert_eq!(latest.get("version").str(), "1.2.3");
    assert_eq!(latest.get("docs").str(), "main");
    let installers = latest.get("installers");
    assert_eq!(
        installers.get("sh").str(),
        "https://oskalang.dev/install.sh"
    );
    assert_eq!(
        installers.get("ps1").str(),
        "https://oskalang.dev/install.ps1"
    );
}

// ---- llms.txt and llms-full.txt -------------------------------------------------------

#[test]
fn llms_txt_indexes_the_markdown_copies_with_summaries() {
    let site = build_ok(&agent_repo());
    assert_eq!(
        site["llms.txt"],
        "# Oska\n\
         \n\
         > A language where authority is explicit.\n\
         \n\
         Oska 1.2.3 (pre-alpha). These docs track `main`. Every page is also served as Markdown: \
         the same path with `.md`. All of them are in [llms-full.txt](https://oskalang.dev/llms-full.txt).\n\
         \n\
         ## Docs\n\
         \n\
         - [Oska](https://oskalang.dev/index.md): A language where authority is explicit.\n\
         - [Glossary](https://oskalang.dev/glossary.md): Capability: a value.\n\
         - [Design](https://oskalang.dev/design.md): How Oska works, and why.\n\
         \n\
         ## Decisions\n\
         \n\
         - [A decision](https://oskalang.dev/adr/0001-a.md): We chose A.\n\
         - [Another](https://oskalang.dev/adr/0002-b.md): We chose B.\n\
         - [Decisions](https://oskalang.dev/adr/index.md)\n\
         \n\
         ## Machine-readable\n\
         \n\
         - [llms-full.txt](https://oskalang.dev/llms-full.txt): every page, as Markdown, in one file\n\
         - [search.json](https://oskalang.dev/search.json): the search index\n\
         - [sitemap.xml](https://oskalang.dev/sitemap.xml): the canonical URL of every page\n\
         - [latest.json](https://oskalang.dev/latest.json): the current version and the installers\n"
    );
}

#[test]
fn a_summary_is_the_first_paragraph_as_plain_text_and_is_cut_short() {
    let long = "word ".repeat(80);
    let doc = format!(
        "# T\n\n> a note\n\n```\ncode\n```\n\n- a list\n\n\
         Text with `code`, **bold**, a [link](https://example.com/x), and\nmore \\* text. {long}\n\nSecond paragraph.\n"
    );
    let site = build_ok(&[("docs/design.md", &doc)]);
    let entry = site["llms.txt"]
        .lines()
        .find(|l| l.starts_with("- [T]"))
        .expect("an entry for the doc")
        .to_string();
    let summary = entry
        .strip_prefix("- [T](https://oskalang.dev/design.md): ")
        .expect("a summary");
    assert!(
        summary.starts_with("Text with code, bold, a link, and more * text. word word"),
        "{summary}"
    );
    assert!(summary.ends_with("..."), "{summary}");
    assert!((199..=200).contains(&summary.chars().count()), "{summary}");
}

#[test]
fn a_doc_with_no_paragraph_has_an_entry_with_no_summary() {
    let site = build_ok(&[("docs/design.md", "# Only a title\n")]);
    assert!(site["llms.txt"].contains("- [Only a title](https://oskalang.dev/design.md)\n"));
}

#[test]
fn llms_full_txt_holds_every_page_s_markdown() {
    let site = build_ok(&agent_repo());
    let full = &site["llms-full.txt"];
    assert!(
        full.starts_with(
            "# Oska: full documentation\n\
             \n\
             Oska 1.2.3 (pre-alpha). Every page of https://oskalang.dev as Markdown, \
             one after another, each after a line `Source:` that names it.\n"
        ),
        "{full}"
    );
    let pages: Vec<&String> = site.keys().filter(|k| k.ends_with(".html")).collect();
    for page in &pages {
        let copy = &site[&twin(page)];
        let section = format!(
            "\n---\n\nSource: https://oskalang.dev{}\n\n{copy}",
            canonical_path(page)
        );
        assert!(full.contains(&section), "{page} is not in:\n{full}");
    }
    assert_eq!(full.matches("\nSource: https://").count(), pages.len());
}

// ---- search.json ------------------------------------------------------------------------

#[test]
fn search_json_has_each_page_s_title_headings_and_text() {
    let site = build_ok(&agent_repo());
    let index = json(&site["search.json"]);
    let entries = index.array();
    let pages = site.keys().filter(|k| k.ends_with(".html")).count();
    assert_eq!(entries.len(), pages);
    let design = entries
        .iter()
        .find(|e| e.get("url").str() == "/design.html")
        .expect("an entry for the design page");
    assert_eq!(design.get("title").str(), "Design");
    let headings: Vec<&str> = design
        .get("headings")
        .array()
        .iter()
        .map(Json::str)
        .collect();
    assert_eq!(headings, ["Values"]);
    let text = design.get("text").str();
    assert!(
        text.starts_with(
            "Design How Oska works, and why. Values Text. adr b up out src here [a](adr/)"
        ),
        "{text}"
    );
    assert!(entries.iter().any(|e| e.get("url").str() == "/adr/"));
}

#[test]
fn search_json_survives_hostile_titles_and_caps_each_text() {
    let body = "word ".repeat(2000);
    let doc = format!("# A \"quoted\" \\ <b>tab</b>\n\n## H\u{e9}llo\t\"x\"\n\n{body}\n");
    let site = build_ok(&[("docs/adr/x.md", &doc)]);
    let index = json(&site["search.json"]);
    let entry = index
        .array()
        .iter()
        .find(|e| e.get("url").str() == "/adr/x.html")
        .expect("an entry");
    assert_eq!(entry.get("title").str(), "A \"quoted\" \\ <b>tab</b>");
    assert_eq!(entry.get("headings").array()[0].str(), "H\u{e9}llo \"x\"");
    assert!(entry.get("text").str().chars().count() <= 4000);
    assert!(!site["search.json"].contains('<'), "markup in the index");
}

// ---- What every page says about itself ----------------------------------------------

/// The `<script>` elements of `page`, as (attributes, content) pairs.
fn scripts_of(page: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = page;
    while let Some(open) = rest.find("<script") {
        rest = &rest[open + "<script".len()..];
        let close = rest.find('>').expect("a script tag that closes");
        let attributes = rest[..close].trim().to_string();
        rest = &rest[close + 1..];
        let end = rest.find("</script>").expect("a script that ends");
        found.push((attributes, rest[..end].to_string()));
        rest = &rest[end..];
    }
    found
}

#[test]
fn every_page_names_its_canonical_url_and_its_markdown_copy() {
    let site = build_ok(&agent_repo());
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        let canonical = format!(
            "<link rel=\"canonical\" href=\"https://oskalang.dev{}\">\n",
            canonical_path(name)
        );
        assert!(page.contains(&canonical), "{name}:\n{page}");
        let copy = format!(
            "<link rel=\"alternate\" type=\"text/markdown\" href=\"/{}\">\n",
            twin(name)
        );
        assert!(page.contains(&copy), "{name}:\n{page}");
    }
}

#[test]
fn every_page_carries_the_version_banner() {
    let site = build_ok(&agent_repo());
    let banner = "<aside aria-label=\"Version\">\n\
                  <p>Oska 1.2.3 (pre-alpha). These docs track <code>main</code>, \
                  which can be ahead of the latest release: see <a href=\"/latest.json\">latest.json</a>.</p>\n\
                  </aside>\n";
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        assert!(
            page.contains(&format!("<body>\n{banner}<header>")),
            "{name}:\n{page}"
        );
    }
}

#[test]
fn the_version_is_read_from_the_manifest_whatever_follows_the_number() {
    for (manifest, shown) in [
        ("version = \"0.1.0\"\n", "0.1.0"),
        (
            "[package]\nname = \"x\"\nrust-version = \"1.75\"\nversion = \"2.10.3-rc.1+build5\"\n",
            "2.10.3",
        ),
        ("version = \"<script>\"\n", "0.0.0"),
        ("name = \"no version\"\n", "0.0.0"),
    ] {
        let site = build_ok(&[("Cargo.toml", manifest)]);
        let latest = json(&site["latest.json"]);
        assert_eq!(latest.get("version").str(), shown, "{manifest}");
        assert!(site["index.html"].contains(&format!("<p>Oska {shown} (pre-alpha).")));
    }
}

#[test]
fn only_the_home_page_has_structured_data_and_it_describes_the_software() {
    let site = build_ok(&agent_repo());
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        let linked: Vec<_> = scripts_of(page)
            .into_iter()
            .filter(|(attributes, _)| attributes == "type=\"application/ld+json\"")
            .collect();
        if name == "index.html" {
            assert_eq!(linked.len(), 1, "{page}");
        } else {
            assert!(linked.is_empty(), "{name} has structured data");
        }
    }
    let home = scripts_of(&site["index.html"]);
    let (_, data) = home
        .iter()
        .find(|(a, _)| a.starts_with("type="))
        .expect("structured data");
    let data = json(data);
    assert_eq!(data.get("@context").str(), "https://schema.org");
    assert_eq!(data.get("@type").str(), "SoftwareSourceCode");
    assert_eq!(data.get("name").str(), "Oska");
    assert_eq!(data.get("version").str(), "1.2.3");
    assert_eq!(
        data.get("license").str(),
        "https://spdx.org/licenses/Apache-2.0.html"
    );
    assert_eq!(data.get("url").str(), "https://oskalang.dev/");
    let product = data.get("targetProduct");
    assert_eq!(
        product.get("installUrl").str(),
        "https://oskalang.dev/install.sh"
    );
    assert_eq!(
        product.get("additionalProperty").get("value").str(),
        "curl -fsSL https://oskalang.dev/install.sh | sh"
    );
}

#[test]
fn the_structured_data_licence_is_the_workspace_s() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .expect("the workspace manifest");
    assert!(
        manifest.contains("license = \"Apache-2.0\""),
        "the generator names Apache-2.0; update it with the manifest"
    );
}

#[test]
fn the_only_scripts_are_the_search_script_and_the_home_page_s_data() {
    let site = build_ok(&agent_repo());
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        for (attributes, _) in scripts_of(page) {
            assert!(
                attributes == "src=\"/search.js\"" || attributes == "type=\"application/ld+json\"",
                "{name} has <script {attributes}>"
            );
        }
        assert!(
            page.contains("<script src=\"/search.js\"></script>\n</body>"),
            "{name} should load the search script last:\n{page}"
        );
    }
}

#[test]
fn the_search_script_is_copied_as_it_is() {
    let site = build_ok(&[("site/search.js", "console.log(\"<&>\");\n")]);
    assert_eq!(site["search.js"], "console.log(\"<&>\");\n");
}

// ---- The search script -------------------------------------------------------------------

/// What `search(index, query)` in `site/search.js` returns, as the titles
/// of the pages it finds, run under Node. `None` if Node is not installed.
fn search_titles(index: &str, query: &str) -> Option<Vec<String>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/search.js");
    let run = std::process::Command::new("node")
        .args([
            "-e",
            "const { search } = require(process.argv[1]); \
             const found = search(JSON.parse(process.argv[2]), process.argv[3]); \
             console.log(JSON.stringify(found.map((p) => p.title)));",
        ])
        .arg(script)
        .arg(index)
        .arg(query)
        .output();
    let Ok(output) = run else {
        eprintln!("skipping: Node is not installed");
        return None;
    };
    assert!(
        output.status.success(),
        "search.js failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let titles = json(&String::from_utf8_lossy(&output.stdout));
    Some(titles.array().iter().map(|t| t.str().to_string()).collect())
}

const SEARCH_INDEX: &str = r#"[
  {"url": "/a.html", "title": "Capabilities", "headings": ["Narrowing"], "text": "authority you hand a function"},
  {"url": "/b.html", "title": "Design", "headings": ["Capabilities and authority"], "text": "the language"},
  {"url": "/c.html", "title": "Glossary", "headings": [], "text": "a capability is a value that grants authority"}
]"#;

#[test]
fn the_search_script_ranks_title_matches_above_headings_above_text() {
    let Some(found) = search_titles(SEARCH_INDEX, "capabilit") else {
        return;
    };
    assert_eq!(found, ["Capabilities", "Design", "Glossary"]);
}

#[test]
fn the_search_script_needs_every_word_and_ignores_case() {
    let Some(found) = search_titles(SEARCH_INDEX, "AUTHORITY value") else {
        return;
    };
    assert_eq!(found, ["Glossary"]);
    let Some(found) = search_titles(SEARCH_INDEX, "   ") else {
        return;
    };
    assert!(found.is_empty());
    let Some(found) = search_titles(SEARCH_INDEX, "zzz") else {
        return;
    };
    assert!(found.is_empty());
}

// ---- The real docs -----------------------------------------------------------------------

/// The repository's own inputs to the generator, as it reads them.
fn real_inputs() -> BTreeMap<String, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut inputs: BTreeMap<String, String> = read_tree(&root.join("docs"))
        .into_iter()
        .map(|(path, text)| (format!("docs/{path}"), text))
        .collect();
    for (path, text) in read_tree(&root.join("site/agent-setup")) {
        inputs.insert(format!("site/agent-setup/{path}"), text);
    }
    for file in [
        "README.md",
        "GLOSSARY.md",
        "Cargo.toml",
        "site/style.css",
        "site/search.js",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect("a repository file");
        inputs.insert(file.to_string(), text);
    }
    inputs
}

#[test]
fn the_real_docs_build_every_agent_output_with_no_dead_link() {
    let built = build(&real_inputs());
    assert_succeeded(&built.outcome);
    let site = built.site;
    assert_site_inert(&site);
    let pages: Vec<&String> = site.keys().filter(|k| k.ends_with(".html")).collect();
    assert!(pages.len() > 20, "{pages:?}");
    for page in &pages {
        assert!(site.contains_key(&twin(page)), "{page} has no copy");
    }
    // Every output parses, and covers every page.
    assert_eq!(json(&site["search.json"]).array().len(), pages.len());
    assert_eq!(
        json(&site["latest.json"]).get("installers").get("sh").str(),
        "https://oskalang.dev/install.sh"
    );
    assert_eq!(site["sitemap.xml"].matches("<loc>").count(), pages.len());
    assert_eq!(
        site["llms-full.txt"].matches("\nSource: https://").count(),
        pages.len()
    );
    // Every link to a copy, from a copy or from llms.txt, goes somewhere.
    let site_path = |url: &str| -> String {
        let path = url.split('#').next().unwrap_or("");
        path.trim_start_matches('/').to_string()
    };
    for (name, text) in site.iter().filter(|(n, _)| n.ends_with(".md")) {
        for destination in text.split("](/").skip(1) {
            let url = destination.split(')').next().unwrap_or("");
            assert!(
                site.contains_key(&site_path(url)),
                "{name} links to /{url}, which is not published"
            );
        }
    }
    for line in site["llms.txt"].lines().filter(|l| l.starts_with("- [")) {
        let url = line
            .split("](https://oskalang.dev/")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("an entry with a URL");
        assert!(site.contains_key(url), "llms.txt lists /{url}");
    }
    assert!(
        site["llms.txt"].contains("## Guides"),
        "{}",
        site["llms.txt"]
    );
}

#[test]
fn a_section_with_its_own_index_doc_gets_no_second_index() {
    let site = build_ok(&[("docs/guide/index.md", "# The guides\n\nStart here.\n")]);
    assert!(site["guide/index.html"].contains("<h1 id=\"the-guides\">The guides</h1>"));
    assert_eq!(site["guide/index.md"], "# The guides\n\nStart here.\n");
    assert_eq!(site["sitemap.xml"].matches("/guide/</loc>").count(), 1);
    assert_eq!(site["search.json"].matches("\"/guide/\"").count(), 1);
}

#[test]
fn search_text_leaves_out_quote_marks_and_table_borders() {
    let doc = "# T\n\n> Quoted *words*.\n\n| Path | Use |\n|---|:-:|\n| `a` | b |\n\n---\n\nEnd.\n";
    let site = build_ok(&[("docs/design.md", doc)]);
    let index = json(&site["search.json"]);
    let entry = index
        .array()
        .iter()
        .find(|e| e.get("url").str() == "/design.html")
        .expect("an entry");
    assert_eq!(entry.get("text").str(), "T Quoted words. Path Use a b End.");
}

#[test]
fn the_code_pages_are_in_the_outputs_agents_read() {
    let site = codes_site();
    assert!(site["sitemap.xml"].contains("<loc>https://oskalang.dev/codes/E0200.html</loc>"));
    assert!(site["sitemap.xml"].contains("<loc>https://oskalang.dev/codes/</loc>"));
    assert!(site["llms.txt"].contains("[codes.json](https://oskalang.dev/codes.json)"));
    assert!(site["llms-full.txt"].contains("Source: https://oskalang.dev/codes/E0200.html"));
    assert!(site["search.json"].contains("\"url\": \"/codes/E0200.html\""));
    let copy = &site["codes/E0200.md"];
    assert!(copy.starts_with("# E0200\n\n"), "{copy}");
    assert!(copy.contains("```oska\nfn f(x: Strng) {}\n```"), "{copy}");
    let index = &site["codes/index.md"];
    assert!(
        index.contains("- [E0200](https://oskalang.dev/codes/E0200.md): Unknown name"),
        "{index}"
    );
    let plain = build_ok(&[]);
    assert!(!plain["llms.txt"].contains("codes.json"));
}

// ---- The agent-setup prompt (spec #70, ticket #80) --------------------------------------

/// A prompt with markup in it, and the files that go with it, as they
/// would be in `site/agent-setup/`.
const SETUP_FILES: &[(&str, &str)] = &[
    (
        "site/agent-setup/prompt.md",
        "# Set up <Oska> & \"go\"\n\nInstall it.\n\n```sh\ncurl -fsSL https://oskalang.dev/install.sh | sh\n```\n",
    ),
    (
        "site/agent-setup/AGENTS.template.md",
        "## Working with Oska\n\nText.\n",
    ),
    ("site/agent-setup/CLAUDE.template.md", "@AGENTS.md\n"),
    ("site/agent-setup/claude-settings.json", "{\"hooks\": {}}\n"),
    ("site/agent-setup/oska-check.sh", "#!/bin/sh\nexit 0\n"),
];

/// [`agent_repo`] with an install guide and the setup files, built.
fn setup_site() -> BTreeMap<String, String> {
    let mut inputs = agent_repo();
    inputs.push((
        "docs/guide/install.md",
        "# Install\n\nBuild it.\n\n## Build from source\n\nRun cargo.\n",
    ));
    inputs.extend_from_slice(SETUP_FILES);
    build_ok(&inputs)
}

#[test]
fn the_prompt_is_published_as_markdown_and_plain_text_exactly_as_written() {
    let site = setup_site();
    let prompt = SETUP_FILES[0].1;
    assert_eq!(site["agent-setup.md"], prompt);
    assert_eq!(site["agent-setup.txt"], prompt);
}

#[test]
fn the_templates_hook_and_script_are_published_byte_for_byte() {
    let site = setup_site();
    for (path, text) in &SETUP_FILES[1..] {
        let name = path.replace("site/", "");
        assert_eq!(site[&name], *text, "{name}");
    }
}

#[test]
fn the_prompt_page_shows_the_prompt_as_escaped_text_with_a_copy_button() {
    let site = setup_site();
    let page = &site["agent-setup.html"];
    assert!(page.contains("<pre id=\"agent-setup-prompt\">"), "{page}");
    assert!(
        page.contains("# Set up &lt;Oska&gt; &amp; &quot;go&quot;\n"),
        "{page}"
    );
    assert!(!page.contains("<Oska>"));
    assert!(page.contains("</pre>"));
    // The button is hidden until the script has shown it, so without
    // JavaScript the page has no button that does nothing.
    assert!(
        page.contains(
            "<button type=\"button\" data-copy=\"agent-setup-prompt\" hidden=\"hidden\">Copy</button>"
        ),
        "{page}"
    );
}

#[test]
fn the_prompt_page_links_the_raw_prompt_and_every_file() {
    let site = setup_site();
    let page = &site["agent-setup.html"];
    for href in [
        "/agent-setup.md",
        "/agent-setup.txt",
        "/agent-setup/AGENTS.template.md",
        "/agent-setup/CLAUDE.template.md",
        "/agent-setup/claude-settings.json",
        "/agent-setup/oska-check.sh",
    ] {
        assert!(page.contains(&format!("href=\"{href}\"")), "{href}");
    }
    assert!(page.contains("Read https://oskalang.dev/agent-setup.md and follow it"));
}

#[test]
fn the_home_page_and_the_install_guide_carry_the_prompt_after_their_introduction() {
    let site = setup_site();
    for (name, intro, next) in [
        (
            "index.html",
            "A language where authority is explicit.",
            "<h2",
        ),
        (
            "guide/install.html",
            "Build it.",
            "<h2 id=\"build-from-source\">",
        ),
    ] {
        let page = &site[name];
        let block = page
            .find("<section class=\"agent-setup\"")
            .unwrap_or_else(|| panic!("{name} has no prompt block:\n{page}"));
        assert!(page[..block].contains(intro), "{name}");
        assert!(
            page[block..].contains("<pre id=\"agent-setup-prompt\">"),
            "{name}"
        );
        assert!(
            page[block..].contains("hidden=\"hidden\">Copy</button>"),
            "{name}"
        );
        assert!(page[block..].contains(next), "{name}: the rest follows");
    }
    assert!(!site["glossary.html"].contains("class=\"agent-setup\""));
    assert!(!site["design.html"].contains("class=\"agent-setup\""));
}

#[test]
fn a_page_shows_the_prompt_once() {
    let site = setup_site();
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        assert!(
            page.matches("id=\"agent-setup-prompt\"").count() <= 1,
            "{name}"
        );
    }
}

#[test]
fn the_markdown_copies_of_the_home_page_and_install_guide_point_at_the_prompt() {
    let site = setup_site();
    for name in ["index.md", "guide/install.md"] {
        assert!(
            site[name].contains("https://oskalang.dev/agent-setup.md"),
            "{name}:\n{}",
            site[name]
        );
    }
    assert!(!site["design.md"].contains("agent-setup"));
}

#[test]
fn llms_txt_lists_the_prompt_near_the_top_and_once() {
    let site = setup_site();
    let llms = &site["llms.txt"];
    let group = llms
        .find("## Set up an agent\n")
        .expect("a group for the prompt");
    assert!(group < llms.find("## Docs").expect("the docs"), "{llms}");
    for line in [
        "- [Agent setup prompt](https://oskalang.dev/agent-setup.md): ",
        "- [agent-setup.txt](https://oskalang.dev/agent-setup.txt): ",
        "- [AGENTS.md template](https://oskalang.dev/agent-setup/AGENTS.template.md): ",
        "- [CLAUDE.md template](https://oskalang.dev/agent-setup/CLAUDE.template.md): ",
        "- [Claude Code hook settings](https://oskalang.dev/agent-setup/claude-settings.json): ",
        "- [Claude Code hook script](https://oskalang.dev/agent-setup/oska-check.sh): ",
    ] {
        assert!(llms.contains(line), "{line}\n{llms}");
    }
    assert_eq!(
        llms.matches("oskalang.dev/agent-setup.md").count(),
        1,
        "{llms}"
    );
}

#[test]
fn the_prompt_page_is_a_page_like_any_other() {
    let site = setup_site();
    assert!(site["sitemap.xml"].contains("<loc>https://oskalang.dev/agent-setup.html</loc>"));
    assert!(site["llms-full.txt"].contains("Source: https://oskalang.dev/agent-setup.html\n"));
    assert!(site["search.json"].contains("\"url\": \"/agent-setup.html\""));
    assert!(
        site["design.html"].contains("href=\"/agent-setup.html\""),
        "it is in the navigation"
    );
}

#[test]
fn without_a_setup_directory_there_is_no_prompt_anywhere() {
    let site = build_ok(&agent_repo());
    assert!(!site.contains_key("agent-setup.html"));
    assert!(!site.contains_key("agent-setup.md"));
    assert!(!site.contains_key("agent-setup.txt"));
    assert!(!site["llms.txt"].contains("agent-setup"));
    assert!(!site["index.html"].contains("agent-setup"));
}

/// Load `site/search.js` into a page with one Copy button, in Node, with a
/// clipboard given as a JavaScript expression, click the button, and say
/// what happened. `None` if Node is not installed.
fn click_copy(clipboard: &str) -> Option<Json> {
    const DRIVER: &str = r#"
const prompt = { textContent: "THE PROMPT" };
const button = {
  hidden: true,
  textContent: "Copy",
  getAttribute: (name) => (name === "data-copy" ? "agent-setup-prompt" : null),
  addEventListener: (type, fn) => { button.click = fn; },
};
global.document = {
  querySelector: () => null,
  querySelectorAll: (selector) => (selector === "button[data-copy]" ? [button] : []),
  getElementById: (id) => (id === "agent-setup-prompt" ? prompt : null),
};
Object.defineProperty(globalThis, "navigator", {
  value: { clipboard: eval("(" + process.argv[2] + ")") },
  configurable: true,
});
global.setTimeout = () => 0;
require(process.argv[1]);
const shown = !button.hidden;
Promise.resolve(button.click()).then(() => {
  console.log(JSON.stringify({ shown, label: button.textContent, wrote: global.written || "" }));
});
"#;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/search.js");
    let run = std::process::Command::new("node")
        .args(["-e", DRIVER])
        .arg(script)
        .arg(clipboard)
        .output();
    let Ok(output) = run else {
        eprintln!("skipping: Node is not installed");
        return None;
    };
    assert!(
        output.status.success(),
        "search.js failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(json(&String::from_utf8_lossy(&output.stdout)))
}

#[test]
fn the_script_shows_the_copy_button_and_copies_the_prompt_text() {
    let Some(result) =
        click_copy("{ writeText: (t) => { global.written = t; return Promise.resolve(); } }")
    else {
        return;
    };
    assert_eq!(result.get("shown"), &Json::Bool(true));
    assert_eq!(result.get("wrote").str(), "THE PROMPT");
    assert_eq!(result.get("label").str(), "Copied");
}

#[test]
fn the_script_says_so_when_the_clipboard_is_not_available() {
    let Some(result) = click_copy("undefined") else {
        return;
    };
    assert_eq!(result.get("label").str(), "Select the text and copy it");
}

// ---- Conformance of the real site (spec #70, ticket #79) --------------------------------
//
// These read the pages the way a crawler or a browser without JavaScript
// would, on the repository's own docs. Together with
// `the_real_docs_build_every_agent_output_with_no_dead_link` (every page
// has a copy, every link between copies resolves) and
// `crates/oskac/tests/catalog.rs` (every E and P code has a page), they
// are the real-docs check of ticket #79. CI runs them with
// `cargo test --all`.

/// One start tag of a page: its element and its attributes, decoded.
struct Tag {
    element: String,
    attributes: Vec<(String, String)>,
}

impl Tag {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The start tags of `page`, in order. Pages have passed
/// [`assert_inert`], so every attribute is double-quoted.
fn tags_of(page: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut rest = page;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest.find('>').expect("a tag that closes");
        let inside = &rest[..close];
        rest = &rest[close + 1..];
        if inside.starts_with('/') || inside.starts_with('!') {
            continue;
        }
        let element: String = inside
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut attributes = Vec::new();
        let mut text = inside[element.len()..].trim_start();
        while let Some(eq) = text.find("=\"") {
            let value_and_rest = &text[eq + 2..];
            let end = value_and_rest.find('"').expect("an attribute that ends");
            attributes.push((
                text[..eq].to_string(),
                decode_references(&value_and_rest[..end]),
            ));
            text = value_and_rest[end + 1..].trim_start();
        }
        tags.push(Tag {
            element,
            attributes,
        });
    }
    tags
}

/// The real site: the repository's docs through the generator. It lacks
/// the files `scripts/release/stage-site.sh` adds ([`STAGED`]).
fn real_site() -> BTreeMap<String, String> {
    let built = build(&real_inputs());
    assert_succeeded(&built.outcome);
    assert_site_inert(&built.site);
    built.site
}

/// Files `stage-site.sh` publishes after the generator has run.
const STAGED: &[&str] = &["install.sh", "install.ps1", "oska-release.pub"];

/// The file in `site` that the URL `target`, found on the page `from`,
/// names, or `None` if it names nothing the site publishes. `target` has
/// no fragment.
fn resolve(site: &BTreeMap<String, String>, from: &str, target: &str) -> Option<String> {
    let target = target
        .strip_prefix("https://oskalang.dev")
        .unwrap_or(target);
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        let mut dir: Vec<&str> = from.split('/').collect();
        dir.pop();
        dir
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let mut path = parts.join("/");
    if target.ends_with('/') || target.is_empty() {
        path = if path.is_empty() {
            "index.html".to_string()
        } else {
            format!("{path}/index.html")
        };
    }
    let published = site.contains_key(&path) || STAGED.contains(&path.as_str());
    published.then_some(path)
}

/// The URL a tag links to or loads, if it does.
fn url_of(tag: &Tag) -> Option<&str> {
    match tag.element.as_str() {
        "a" | "link" => tag.attribute("href"),
        "img" | "script" => tag.attribute("src"),
        _ => None,
    }
}

#[test]
fn every_link_in_every_real_page_goes_to_a_published_page_and_anchor() {
    let site = real_site();
    let mut checked = 0;
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        for tag in tags_of(page) {
            let Some(url) = url_of(&tag) else { continue };
            if url.starts_with("mailto:")
                || (url.starts_with("http") && !url.starts_with("https://oskalang.dev"))
            {
                continue;
            }
            let (path, fragment) = url.split_once('#').unwrap_or((url, ""));
            let target = if path.is_empty() {
                name.clone()
            } else {
                resolve(&site, name, path)
                    .unwrap_or_else(|| panic!("{name} links to {url}, which is not published"))
            };
            checked += 1;
            if fragment.is_empty() || !target.ends_with(".html") {
                continue;
            }
            assert!(
                tags_of(&site[&target])
                    .iter()
                    .any(|t| t.attribute("id") == Some(fragment)),
                "{name} links to {url}, but {target} has no anchor `{fragment}`"
            );
        }
    }
    assert!(checked > 1000, "only {checked} links were checked");
}

/// The only origin pages may load anything from: Fontsource, at the
/// pinned versions the CSP names.
const ASSET_ALLOWLIST: &[&str] = &[
    "https://cdn.jsdelivr.net/npm/@fontsource/inter@5.3.0/",
    "https://cdn.jsdelivr.net/npm/@fontsource/jetbrains-mono@5.3.0/",
];

#[test]
fn every_external_asset_is_on_the_allowlist_pinned_with_sri_and_named_by_the_csp() {
    let site = real_site();
    let mut assets = 0;
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        let mut csp = None;
        for tag in tags_of(page) {
            if tag.element == "meta" && tag.attribute("http-equiv").is_some() {
                csp = tag.attribute("content").map(str::to_string);
            }
            let rel = tag.attribute("rel");
            let loads = tag.element != "a" && rel != Some("canonical") && rel != Some("alternate");
            let Some(url) = url_of(&tag).filter(|u| loads && u.contains("://")) else {
                continue;
            };
            assets += 1;
            let Some(origin) = ASSET_ALLOWLIST.iter().find(|ok| url.starts_with(**ok)) else {
                panic!("{name} loads {url}, which is not on the allowlist");
            };
            let integrity = tag.attribute("integrity").unwrap_or("");
            let digest = integrity.strip_prefix("sha384-").unwrap_or("");
            assert!(
                digest.len() == 64
                    && digest
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/'),
                "{name}: {url} has no sha384 integrity: `{integrity}`"
            );
            assert!(
                tag.attribute("crossorigin").is_some(),
                "{name}: {url} has no crossorigin"
            );
            assert!(
                csp.as_deref().is_some_and(|c| c.contains(origin)),
                "{name}: the CSP does not name the origin of {url}"
            );
        }
        // The CSP names no origin the allowlist does not.
        for source in csp.expect("a CSP").split_whitespace() {
            let source = source.trim_end_matches(';');
            if source.starts_with("https://") || source.starts_with("http://") {
                assert!(
                    ASSET_ALLOWLIST.contains(&source),
                    "{name}: the CSP allows {source}"
                );
            }
        }
    }
    assert!(assets > 100, "only {assets} external assets were checked");
    // The stylesheet and the search script load nothing of their own:
    // no import, no `url(`, and, past the comment that names the site,
    // no absolute URL.
    for file in ["style.css", "search.js"] {
        let text = &site[file];
        let code = text.split_once('\n').map_or("", |(_, rest)| rest);
        assert!(
            !text.contains("@import")
                && !text.contains("url(")
                && !code.contains("http://")
                && !code.contains("https://"),
            "{file} names another origin"
        );
    }
}

#[test]
fn every_real_page_reads_in_full_without_javascript() {
    let site = real_site();
    // 1. The only scripts are the search enhancement and the JSON-LD
    //    data block, which does not run.
    // 2. The only thing hidden until a script shows it is a copy button;
    //    the text it copies is on the page.
    // 3. There is no form or input for a script to drive.
    for (name, page) in site.iter().filter(|(n, _)| n.ends_with(".html")) {
        let tags = tags_of(page);
        for tag in &tags {
            if tag.element == "script" {
                assert!(
                    tag.attribute("src") == Some("/search.js")
                        || tag.attribute("type") == Some("application/ld+json"),
                    "{name}: an unexpected script {:?}",
                    tag.attributes
                );
            }
            if tag.attribute("hidden").is_some() {
                let target = tag.attribute("data-copy").unwrap_or("");
                assert!(
                    tag.element == "button"
                        && tags
                            .iter()
                            .any(|t| t.element == "pre" && t.attribute("id") == Some(target)),
                    "{name}: `<{}>` is hidden, and not a copy button for text on the page",
                    tag.element
                );
            }
            assert!(
                !matches!(tag.element.as_str(), "form" | "input" | "noscript"),
                "{name}: `<{}>`",
                tag.element
            );
        }
    }
    // 4. Every page can be reached from the home page by following links.
    let mut reached = std::collections::BTreeSet::from(["index.html".to_string()]);
    let mut queue = vec!["index.html".to_string()];
    while let Some(name) = queue.pop() {
        for tag in tags_of(&site[&name]).iter().filter(|t| t.element == "a") {
            let href = tag.attribute("href").unwrap_or("");
            let path = href.split('#').next().unwrap_or("");
            if path.is_empty() || path.starts_with("http") || path.starts_with("mailto:") {
                continue;
            }
            if let Some(target) = resolve(&site, &name, path) {
                if target.ends_with(".html") && reached.insert(target.clone()) {
                    queue.push(target);
                }
            }
        }
    }
    let unreachable: Vec<&String> = site
        .keys()
        .filter(|k| k.ends_with(".html") && !reached.contains(*k))
        .collect();
    assert!(
        unreachable.is_empty(),
        "not linked from anywhere: {unreachable:?}"
    );
}
