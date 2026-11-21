//! The API reference on the site (#294).
//!
//! The Pages workflow writes what `oskac doc --format=json` prints to
//! `site/api.json` just before the build. The generator reads it, with one
//! read narrowed to that file, and publishes `/api/`, a page per module
//! with a Markdown copy, `api.json`, and entries in `llms.txt`. These
//! tests feed it the `api` golden case's fixture (two modules, an item that
//! only checks, and hostile text) and malformed files. The real reference
//! is tested in `oskac`'s own tests, which can run `oskac doc`.
//!
//! The generator runs through the interpreter on a [`FakeHost`], and
//! natively on aarch64 and x86_64 Linux (`common`).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::inert::decode_references;
use common::site::{
    between, build, build_ok, generator_authority, hrefs, llms_group, read_tree, BUILD,
    MINIMAL_REPO, PAGES_WORKFLOW,
};
use oska_syntax::{parse_json, JsonValue};

/// The `api` golden case's inputs.
fn fixture_inputs() -> BTreeMap<String, String> {
    read_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/tests/api/input"))
}

/// The fixture `site/api.json`.
fn fixture_json() -> String {
    fixture_inputs()["site/api.json"].clone()
}

/// The site built from the fixture, asserting that it succeeded and that
/// every page is inert.
fn fixture_site() -> BTreeMap<String, String> {
    let inputs = fixture_inputs();
    let refs: Vec<(&str, &str)> = inputs
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    build_ok(&refs)
}

/// The items of an `{"items": [...]}` text.
fn items_of(json: &str) -> Vec<JsonValue> {
    parse_json(json)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {json}"))
        .get("items")
        .and_then(JsonValue::as_array)
        .expect("an items array")
        .to_vec()
}

fn field<'a>(item: &'a JsonValue, key: &str) -> &'a str {
    item.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_else(|| panic!("no string {key} in {item:?}"))
}

/// The build of [`MINIMAL_REPO`] with `api` as `site/api.json`: it must
/// fail, naming the file and each of `named`, and write no page.
fn assert_api_refused(api: &str, named: &[&str]) {
    let files: BTreeMap<String, String> = MINIMAL_REPO
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .chain([("site/api.json".to_string(), api.to_string())])
        .collect();
    let (outcome, site) = build(&files);
    assert_eq!(
        outcome.exit_code, 1,
        "the build should fail on {api:?}: {outcome:#?}"
    );
    for name in ["site/api.json"].iter().chain(named) {
        assert!(
            outcome.stderr.contains(name),
            "the error for {api:?} should name {name:?}: {}",
            outcome.stderr
        );
    }
    assert!(
        !site.keys().any(|k| k.ends_with(".html")),
        "a failed build should write no page: {:?}",
        site.keys()
    );
}

/// One item of the reference as `oskac doc --format=json` writes it.
fn item(module: &str, name: &str, kind: &str, url: &str) -> String {
    format!(
        "{{\"module\":\"{module}\",\"name\":\"{name}\",\"kind\":\"{kind}\",\"signature\":\"\",\"description\":\"\",\"authority\":\"pure\",\"runs\":true,\"url\":\"{url}\"}}"
    )
}

/// The module item of `prelude`, which a well-formed file needs.
fn prelude() -> String {
    item(
        "prelude",
        "prelude",
        "module",
        "https://oskalang.dev/api/prelude.html",
    )
}

fn reference(items: &[String]) -> String {
    format!("{{\"items\": [\n  {}\n]}}\n", items.join(",\n  "))
}

// ---- Without the file --------------------------------------------------------------

#[test]
fn without_site_api_json_there_is_no_api_reference() {
    let site = build_ok(&[]);
    assert!(
        !site.keys().any(|k| k.starts_with("api/")),
        "{:?}",
        site.keys()
    );
    assert!(!site.contains_key("api.json"));
    assert!(!site["llms.txt"].contains("API reference"));
    assert!(!site["llms.txt"].contains("api.json"));
    assert!(!site["reference.html"].contains("/api/"));
}

// ---- The pages ----------------------------------------------------------------------

#[test]
fn the_index_links_each_module_with_its_description_s_first_paragraph() {
    let site = fixture_site();
    let index = &site["api/index.html"];
    let links = hrefs(between(index, "<main id=\"main-content\">", "</main>"));
    assert!(links.contains(&"/api/prelude.html"), "{links:?}");
    assert!(links.contains(&"/api/std-sql.html"), "{links:?}");
    assert!(links.contains(&"/api.json"), "{links:?}");
    assert!(index.contains("The prelude: what every module sees without <code>use</code>."));
    assert!(!index.contains("A second paragraph"));
    let copy = &site["api/index.md"];
    assert!(copy.starts_with("# API reference\n"), "{copy}");
    assert!(copy.contains(
        "- [prelude](https://oskalang.dev/api/prelude.md): The prelude: what every module sees without `use`.\n"
    ));
    assert!(copy.contains(
        "- [std.sql](https://oskalang.dev/api/std-sql.md): The SQL sink. See <b>this</b>.\n"
    ));
    assert!(site["reference.html"].contains("<a href=\"/api/\">API reference</a>"));
}

/// Every item has a heading whose text is its name and whose anchor is
/// the one in its `url`, on the page its `url` names; that page has a
/// Markdown copy with the same heading.
#[test]
fn every_item_has_a_heading_at_the_anchor_of_its_url() {
    let site = fixture_site();
    for item in items_of(&fixture_json()) {
        let url = field(&item, "url");
        let path = url
            .strip_prefix("https://oskalang.dev/")
            .expect("a URL on the site");
        let (page, anchor) = path.split_once('#').unwrap_or((path, ""));
        let html = site
            .get(page)
            .unwrap_or_else(|| panic!("no page {page} for {url}"));
        let copy = &site[&page.replace(".html", ".md")];
        let name = field(&item, "name");
        if anchor.is_empty() {
            assert_eq!(field(&item, "kind"), "module");
            let title = between(html, "<h1 id=\"", "</h1>");
            assert_eq!(
                decode_references(title.split_once("\">").expect("a heading").1),
                name
            );
            continue;
        }
        let open = format!("<h2 id=\"{anchor}\">");
        assert_eq!(
            html.matches(&open).count(),
            1,
            "{page} should have one heading at #{anchor}"
        );
        let shown = decode_references(between(html, &open, "</h2>"));
        assert_eq!(shown, name, "the heading of {url}");
        assert!(copy.contains("\n## "), "{copy}");
    }
}

#[test]
fn each_item_shows_its_kind_signature_and_authority() {
    let site = fixture_site();
    let page = &site["api/std-sql.html"];
    let entry = between(page, "<h2 id=\"connect\">", "<h2 ");
    assert!(entry.contains("<strong>Kind:</strong> fn"), "{entry}");
    assert!(
        entry.contains("<strong>Authority:</strong> NetConnect (net)"),
        "{entry}"
    );
    assert!(entry.contains(
        "<pre><code>pub fn connect(net: NetConnect, url: String) -&gt; Result&lt;Db, SqlError&gt;\n</code></pre>"
    ));
    assert!(entry.contains("<p>Connect to the database at <code>url</code>.</p>"));
    let copy = &site["api/std-sql.md"];
    assert!(copy.contains(
        "## connect\n\n**Kind:** fn · **Authority:** NetConnect (net)\n\n```oska\npub fn connect(net: NetConnect, url: String) -> Result<Db, SqlError>\n```\n"
    ), "{copy}");
}

#[test]
fn what_only_checks_says_so_and_what_runs_does_not() {
    let site = fixture_site();
    let note = "<strong>Checks but does not run.</strong>";
    // The module, and each of its two items.
    assert_eq!(site["api/std-sql.html"].matches(note).count(), 3);
    assert!(!site["api/prelude.html"].contains(note));
    assert!(site["api/std-sql.html"]
        .contains("<a href=\"/guide/what-works-today.html\">What works today</a>"));
    assert_eq!(
        site["api/std-sql.md"]
            .matches("> **Checks but does not run.**")
            .count(),
        3
    );
}

#[test]
fn hostile_text_in_the_reference_is_shown_as_text() {
    let site = fixture_site();
    let prelude = &site["api/prelude.html"];
    assert!(prelude.contains("Hostile: &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        !prelude.contains("javascript:alert"),
        "an unsafe link is only its text"
    );
    assert!(prelude.contains("a link,"), "{prelude}");
    assert!(prelude.contains("<code>a `tick` span</code>"));
    assert!(prelude.contains("&quot;quotes&quot;, a back\\slash, Ab&lt; / a slash,\tand a tab."));
    // A heading in a description is text, and a fence it leaves open is
    // closed before the next item.
    assert!(prelude.contains("<p># Not a heading</p>"), "{prelude}");
    assert!(!prelude.contains("id=\"not-a-heading\""));
    assert!(prelude
        .contains("<pre><code>an open fence &lt;b&gt;bold&lt;/b&gt;\n## nor this\n</code></pre>"));
    assert!(prelude.contains("<h2 id=\"listpush\">List.push</h2>"));
    // A name is shown as it is, in a heading and in the page's title.
    assert!(prelude.contains("<h2 id=\"eq-for-optiont\">Eq for Option&lt;T&gt;</h2>"));
    assert!(prelude.contains("<h2 id=\"stringto_lowercase\">String.to_lowercase</h2>"));
    let sql = &site["api/std-sql.html"];
    assert!(sql.contains("<h2 id=\"dbquery\">Db.*query*</h2>"), "{sql}");
    assert!(sql.contains(
        "<strong>Authority:</strong> &lt;img src=x onerror=alert(1)&gt; _em_ `tick`</p>"
    ));
    assert!(sql.contains(
        "<pre><code>```&lt;/code&gt;&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;\n</code></pre>"
    ));
    assert!(sql.contains("<p>The SQL sink. See &lt;b&gt;this&lt;/b&gt;.</p>"));
}

#[test]
fn api_json_holds_the_same_items_with_no_raw_angle_bracket() {
    let site = fixture_site();
    let published = &site["api.json"];
    assert_eq!(
        parse_json(published).expect("api.json is JSON"),
        parse_json(&fixture_json()).expect("the fixture is JSON")
    );
    assert!(published.starts_with("{\"items\": [\n  {\"module\":\"prelude\""));
    assert!(!published.contains('<'), "{published}");
    assert!(published.contains("\"runs\":false"));
}

// ---- The agent outputs --------------------------------------------------------------

#[test]
fn llms_txt_lists_the_api_pages_and_api_json() {
    let site = fixture_site();
    let llms = &site["llms.txt"];
    let group = llms_group(llms, "API reference");
    let entries: Vec<&str> = group
        .lines()
        .filter(|l| l.starts_with("- ["))
        .map(|l| {
            l.split("](")
                .nth(1)
                .expect("a link")
                .split(')')
                .next()
                .expect("a URL")
        })
        .collect();
    assert_eq!(
        entries,
        [
            "https://oskalang.dev/api/index.md",
            "https://oskalang.dev/api/prelude.md",
            "https://oskalang.dev/api/std-sql.md"
        ],
        "{group}"
    );
    assert!(group.contains("- [prelude](https://oskalang.dev/api/prelude.md): The prelude: what every module sees without use.\n"), "{group}");
    let data = llms_group(llms, "Machine-readable");
    assert!(
        data.contains(
            "- [api.json](https://oskalang.dev/api.json): every item of the API reference"
        ),
        "{data}"
    );
    // Listed once: not again under Docs.
    assert_eq!(llms.matches("/api/prelude.md").count(), 1);
}

#[test]
fn the_api_pages_are_in_llms_full_the_sitemap_and_the_search_index() {
    let site = fixture_site();
    for page in ["api/", "api/prelude.html", "api/std-sql.html"] {
        let url = format!("https://oskalang.dev/{page}");
        assert!(
            site["llms-full.txt"].contains(&format!("\nSource: {url}\n")),
            "llms-full.txt lacks {url}"
        );
        assert!(site["sitemap.xml"].contains(&format!("<loc>{url}</loc>")));
        assert!(site["search.json"].contains(&format!("\"url\": \"/{page}\"")));
    }
    assert!(site["llms-full.txt"].contains("## List.push\n"));
    let search = parse_json(&site["search.json"]).expect("search.json is JSON");
    let prelude = search
        .as_array()
        .expect("an array")
        .iter()
        .find(|e| field(e, "url") == "/api/prelude.html")
        .expect("the prelude page is indexed");
    let headings: Vec<&str> = prelude
        .get("headings")
        .and_then(JsonValue::as_array)
        .expect("headings")
        .iter()
        .map(|h| h.as_str().expect("a heading"))
        .collect();
    assert_eq!(
        headings,
        ["Eq for Option<T>", "List.push", "String.to_lowercase"]
    );
}

// ---- Malformed files ----------------------------------------------------------------

#[test]
fn a_file_that_is_not_the_reference_s_json_fails_the_build_naming_it() {
    let good = item(
        "prelude",
        "List.push",
        "method",
        "https://oskalang.dev/api/prelude.html#listpush",
    );
    for (api, named) in [
        ("", "expected `{` at character 0"),
        ("{", "expected a string at character 1"),
        ("{\"things\": []}", "\"items\""),
        ("{\"items\": [}", "expected `{` at character 11"),
        ("{\"items\": []} x", "text after the end"),
        (
            "{\"items\": [{\"module\": 1}]}",
            "expected a string, true or false",
        ),
        (
            "{\"items\": [{\"module\": null}]}",
            "expected a string, true or false",
        ),
        (
            "{\"items\": [{\"module\": \"a\\x\"}]}",
            "cannot decode, \\x",
        ),
        ("{\"items\": [{\"module\": \"\\u00e9\"}]}", "cannot decode"),
        (
            "{\"items\": [{\"module\": \"\\ud83d\\ude00\"}]}",
            "cannot decode",
        ),
        ("{\"items\": [{\"module\": \"\\b\"}]}", "cannot decode, \\b"),
        (
            "{\"items\": [{\"module\": \"\\u00zz\"}]}",
            "not hexadecimal",
        ),
        (
            "{\"items\": [{\"module\": \"a\nb\"}]}",
            "a control character",
        ),
        ("{\"items\": [{\"module\": \"a", "does not end"),
        (
            "{\"items\": [{\"module\": [\"a\"]}]}",
            "expected a string, true or false",
        ),
        ("{\"items\": [{}]}", "an empty item"),
    ] {
        assert_api_refused(api, &[named]);
    }
    let missing = prelude().replace(",\"runs\":true", "");
    assert_api_refused(&reference(&[missing]), &["without \"runs\""]);
    let twice = prelude().replace("\"kind\"", "\"name\":\"x\",\"kind\"");
    assert_api_refused(&reference(&[twice]), &["\"name\" twice"]);
    let text_runs = prelude().replace("\"runs\":true", "\"runs\":\"yes\"");
    assert_api_refused(&reference(&[text_runs]), &["\"runs\" is not true or false"]);
    let bool_name = prelude().replace("\"name\":\"prelude\"", "\"name\":true");
    assert_api_refused(&reference(&[bool_name]), &["\"name\" is not a string"]);
    let trailing = format!("{{\"items\": [{}, ]}}", prelude());
    assert_api_refused(&trailing, &["expected `{`"]);
    // The items, though well-formed JSON, must agree with their pages.
    let wrong_url = good.replace("#listpush", "#push");
    assert_api_refused(
        &reference(&[prelude(), wrong_url]),
        &["the url of `List.push`", "#listpush"],
    );
    let other_site = good.replace("https://oskalang.dev/", "https://example.com/");
    assert_api_refused(
        &reference(&[prelude(), other_site]),
        &["the url of `List.push`"],
    );
    assert_api_refused(
        &reference(&[prelude(), good.clone(), good.clone()]),
        &["two items have the url"],
    );
    assert_api_refused(
        &reference(&[prelude(), good.replace("\"method\"", "\"macro\"")]),
        &["unknown kind, `macro`"],
    );
    assert_api_refused(
        &reference(std::slice::from_ref(&good)),
        &["no item of kind `module`"],
    );
    let escape = item(
        "../x",
        "../x",
        "module",
        "https://oskalang.dev/api/..-x.html",
    );
    assert_api_refused(&reference(&[escape]), &["is not a path of letters"]);
    let spaced = good.replace("\"List.push\"", "\" List.push\"");
    assert_api_refused(
        &reference(&[prelude(), spaced]),
        &["which no heading can show"],
    );
}

#[test]
fn an_empty_reference_publishes_none() {
    let site = build_ok(&[("site/api.json", "{\"items\": []}\n")]);
    assert!(!site.contains_key("api/index.html"));
    assert!(!site.contains_key("api.json"));
}

// ---- Authority and the workflow ------------------------------------------------------

#[test]
fn main_narrows_one_new_read_to_site_api_json_and_the_report_shows_it() {
    assert!(BUILD.contains("api: read.restrict_to_file(\"site/api.json\")"));
    let report = generator_authority();
    assert!(
        report.contains("  FsRead -> FileRead: restrict_to_file \"site/api.json\" (bytes "),
        "{report}"
    );
}

/// The reference is the real command's output: the Pages workflow writes
/// it with the `oskac` it just built, before it runs the generator, and
/// nothing touches the file in between; and the smoke check fetches the
/// published `api.json`.
#[test]
fn the_pages_workflow_writes_the_reference_with_the_real_oskac_before_the_build() {
    let doc = "./target/release/oskac doc --format=json > site/api.json";
    let run = "./target/release/oskac run site/build.osk";
    let at_doc = PAGES_WORKFLOW
        .find(doc)
        .unwrap_or_else(|| panic!("pages.yml does not run `{doc}`"));
    let at_run = PAGES_WORKFLOW
        .find(run)
        .unwrap_or_else(|| panic!("pages.yml does not run `{run}`"));
    assert!(at_doc < at_run, "the reference is written after the build");
    assert!(!PAGES_WORKFLOW[at_doc + doc.len()..at_run].contains("api.json"));
    let smoke = between(
        PAGES_WORKFLOW,
        "the files agents and installers fetch are published",
        "done",
    );
    assert!(smoke.contains(" api.json "), "{smoke}");
}
