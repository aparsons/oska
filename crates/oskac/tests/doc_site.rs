// SPDX-License-Identifier: Apache-2.0
//! The whole real API reference on the site (#294): the site generator,
//! `site/build.osk`, fed what `oskac doc --format=json` prints, as the
//! Pages workflow feeds it. `oska-run`'s site tests feed it fixtures and
//! the `std.sql` part; this is the one place the generator reads the whole
//! reference, every item of the prelude and `std` included.
//!
//! The generator runs on the interpreter, on a [`FakeHost`] that holds the
//! few files it needs beside the reference.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use oska_run::FakeHost;
use oska_syntax::{parse_json, JsonValue};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// What `oskac doc --format=json` prints.
fn reference() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["doc", "--format=json"])
        .output()
        .expect("oskac doc should run");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).expect("the reference is UTF-8")
}

/// The site the generator writes from the reference and the smallest
/// repository it builds from.
fn site(reference: &str) -> BTreeMap<String, String> {
    let root = repo_root();
    let mut host = FakeHost::new()
        .with_file("docs/index.md", "# Oska\n")
        .with_file("GLOSSARY.md", "# Glossary\n")
        .with_file("Cargo.toml", "[workspace.package]\nversion = \"1.2.3\"\n")
        .with_file("site/style.css", "body {}\n")
        .with_file("site/search.js", "// search\n")
        .with_file("site/favicon.svg", "<svg></svg>\n")
        .with_file("site/api.json", reference);
    let build = std::fs::read_to_string(root.join("site/build.osk")).expect("the generator");
    let outcome = oska_run::run(&build, &mut host, &["site/build.osk".to_string()]);
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should publish the real reference: {outcome:#?}"
    );
    host.files_under("_site")
}

fn field<'a>(item: &'a JsonValue, key: &str) -> &'a str {
    item.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_else(|| panic!("item {item:?} should have a string `{key}`"))
}

/// `text` with the references `html.text` writes decoded.
fn decoded(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Every item of the real reference has a heading on the page its `url`
/// names, at the anchor its `url` names, whose text is its name; every
/// module's page has a Markdown copy; and `api.json` holds the same items.
#[test]
fn the_site_publishes_every_item_of_the_real_reference_at_its_url() {
    let reference = reference();
    let site = site(&reference);
    let parsed = parse_json(&reference).expect("the reference is JSON");
    let items = parsed
        .get("items")
        .and_then(JsonValue::as_array)
        .expect("an items array");
    assert!(items.len() > 100, "{} items", items.len());
    let mut problems = Vec::new();
    for item in items {
        let url = field(item, "url");
        let name = field(item, "name");
        let path = url
            .strip_prefix("https://oskalang.dev/")
            .expect("a URL on the site");
        let (page, anchor) = path.split_once('#').unwrap_or((path, ""));
        let Some(html) = site.get(page) else {
            problems.push(format!("{url}: no page {page}"));
            continue;
        };
        if !site.contains_key(&page.replace(".html", ".md")) {
            problems.push(format!("{url}: no Markdown copy"));
        }
        if anchor.is_empty() {
            continue;
        }
        let open = format!("<h2 id=\"{anchor}\">");
        let Some(at) = html.find(&open) else {
            problems.push(format!("{url}: no heading at #{anchor}"));
            continue;
        };
        let rest = &html[at + open.len()..];
        let shown = decoded(&rest[..rest.find("</h2>").expect("the heading ends")]);
        if shown != name {
            problems.push(format!("{url}: the heading says {shown:?}, not {name:?}"));
        }
        if html.matches(&open).count() != 1 {
            problems.push(format!("{url}: more than one heading at #{anchor}"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    assert_eq!(
        parse_json(&site["api.json"]).expect("api.json is JSON"),
        parsed,
        "api.json should hold the reference's items"
    );
    for page in [
        "api/index.html",
        "api/prelude.html",
        "api/std-html.html",
        "api/std-sql.html",
        "api/std-proc.html",
    ] {
        assert!(site.contains_key(page), "no {page}");
    }
}
