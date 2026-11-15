//! Every link the site publishes goes somewhere (#186).
//!
//! The site generator, `site/build.osk`, resolves each relative link in a
//! doc once, and the HTML page, its Markdown copy and `llms-full.txt` all
//! get the result: a published page, or, for a file in the repository the
//! site does not publish, that file on GitHub. A link that leaves the
//! repository, or has no destination, fails the build.
//!
//! The link checker, `site/check-links.osk`, reads the built site and
//! fails on any link that goes nowhere: a missing page, a missing anchor,
//! or a GitHub link to a file the repository does not track.
//!
//! Both run through the interpreter on a [`FakeHost`], and natively on
//! aarch64 Linux (`common`).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use oska_run::{FakeHost, RunOutcome};

/// The generator's source.
const BUILD: &str = include_str!("../../../site/build.osk");

/// The link checker's source.
const CHECK: &str = include_str!("../../../site/check-links.osk");

/// Where a file the site does not publish is linked: on GitHub, on `main`.
const BLOB: &str = "https://github.com/aparsons/oska/blob/main/";

/// The files every repository the generator reads must have.
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

/// Run the generator on [`MINIMAL_REPO`] with `inputs` added: how it
/// ended, and the files it wrote under `_site/`.
fn generate(inputs: &[(&str, &str)]) -> (RunOutcome, BTreeMap<String, String>) {
    let files: BTreeMap<&str, &str> = MINIMAL_REPO.iter().chain(inputs).copied().collect();
    let mut host = files.iter().fold(FakeHost::new(), |host, (path, text)| {
        host.with_file(path, text)
    });
    let outcome = common::run_differential(&mut host, BUILD, &[]);
    (outcome, host.files_under("_site"))
}

fn succeeded(outcome: &RunOutcome) -> bool {
    outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none()
}

/// The site the generator writes from [`MINIMAL_REPO`] and `inputs`,
/// asserting that it succeeded.
fn site_of(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let (outcome, site) = generate(inputs);
    assert!(
        succeeded(&outcome),
        "the generator should succeed: {outcome:#?}"
    );
    site
}

/// Assert that the generator refuses `inputs`, with a message that names
/// each of `named`, and writes no page.
fn assert_build_fails(inputs: &[(&str, &str)], named: &[&str]) {
    let (outcome, site) = generate(inputs);
    assert_eq!(outcome.exit_code, 1, "the build should fail: {outcome:#?}");
    for name in named {
        assert!(
            outcome.stderr.contains(name),
            "the error should name {name:?}: {}",
            outcome.stderr
        );
    }
    assert!(
        !site.keys().any(|k| k.ends_with(".html")),
        "a failed build should write no page: {:?}",
        site.keys()
    );
}

// ---- The generator ---------------------------------------------------------------

#[test]
fn a_link_to_an_unpublished_repository_file_goes_to_github_in_every_output() {
    let site = site_of(&[
        (
            "README.md",
            "# Oska\n\nApache-2.0. See [LICENSE](LICENSE) and [the shop](examples/shopping).\n",
        ),
        (
            "docs/design.md",
            "# Design\n\nThe [checker](../crates/oska-check/src/lib.rs#L10 \"source\").\n",
        ),
    ]);
    let license = format!("{BLOB}LICENSE");
    let shop = format!("{BLOB}examples/shopping");
    let checker = format!("{BLOB}crates/oska-check/src/lib.rs#L10");
    assert!(
        site["index.html"].contains(&format!(
            "<a href=\"{license}\">LICENSE</a> and <a href=\"{shop}\">the shop</a>"
        )),
        "{}",
        site["index.html"]
    );
    assert!(
        site["design.html"].contains(&format!("<a href=\"{checker}\">checker</a>")),
        "{}",
        site["design.html"]
    );
    let readme_copy = format!("[LICENSE]({license}) and [the shop]({shop})");
    let design_copy = format!("[checker]({checker} \"source\")");
    assert!(
        site["index.md"].contains(&readme_copy),
        "{}",
        site["index.md"]
    );
    assert!(
        site["design.md"].contains(&design_copy),
        "{}",
        site["design.md"]
    );
    assert!(site["llms-full.txt"].contains(&readme_copy));
    assert!(site["llms-full.txt"].contains(&design_copy));
}

#[test]
fn a_link_to_the_repository_root_goes_to_the_repository() {
    let site = site_of(&[("docs/design.md", "# Design\n\nThe [source](../).\n")]);
    assert!(site["design.html"].contains("<a href=\"https://github.com/aparsons/oska\">source</a>"));
    assert!(site["design.md"].contains("[source](https://github.com/aparsons/oska)"));
}

#[test]
fn a_link_that_leaves_the_repository_fails_the_build_naming_the_page_and_the_link_text() {
    assert_build_fails(
        &[(
            "docs/design.md",
            "# Design\n\nFine: [readme](../README.md). Not: [the passwords](../../etc/passwd).\n",
        )],
        &["docs/design.md", "the passwords", "../../etc/passwd"],
    );
}

#[test]
fn an_empty_link_fails_the_build_naming_the_page_and_the_link_text() {
    assert_build_fails(
        &[("docs/adr/0001-a.md", "# A\n\nSee [nothing at all]().\n")],
        &["docs/adr/0001-a.md", "nothing at all"],
    );
}

#[test]
fn a_bad_link_in_code_is_only_text_and_does_not_fail_the_build() {
    let site = site_of(&[(
        "docs/design.md",
        "# Design\n\nWrite `[x](../../out)` for a link.\n\n```\n[y](../../out)\n```\n",
    )]);
    assert!(site["design.md"].contains("`[x](../../out)`"));
    assert!(site["design.md"].contains("```\n[y](../../out)\n```"));
}

#[test]
fn an_image_and_an_unsafe_link_keep_their_text_in_the_copy() {
    let site = site_of(&[(
        "docs/design.md",
        "# Design\n\n![a diagram](pic.png) [run](javascript:alert(1)) [abs](/etc/passwd)\n",
    )]);
    assert!(
        site["design.md"]
            .contains("![a diagram](pic.png) [run](javascript:alert(1)) [abs](/etc/passwd)"),
        "{}",
        site["design.md"]
    );
    assert!(site["design.html"].contains("<img src=\"pic.png\" alt=\"a diagram\"> run abs"));
}

#[test]
fn the_guide_examples_are_published_with_copies_and_linked_from_the_guides() {
    let site = site_of(&[
        (
            "docs/guide/index.md",
            "# Guides\n\n- [hello](examples/hello.md)\n",
        ),
        (
            "docs/guide/examples/hello.md",
            "# Example: hello\n\nThe smallest program. Back to [the guides](../index.md), \
             or [the source](../../../examples/hello.osk).\n",
        ),
    ]);
    assert!(site.contains_key("guide/examples/hello.html"));
    assert!(site["guide/examples/hello.md"].starts_with("# Example: hello\n"));
    assert!(site["guide/index.html"].contains("<a href=\"/guide/examples/hello.html\">hello</a>"));
    assert!(site["guide/index.md"].contains("[hello](/guide/examples/hello.md)"));
    let page = &site["guide/examples/hello.html"];
    assert!(
        page.contains("<a href=\"/guide/\">the guides</a>"),
        "{page}"
    );
    assert!(page.contains(&format!(
        "<a href=\"{BLOB}examples/hello.osk\">the source</a>"
    )));
    assert!(site["sitemap.xml"].contains("https://oskalang.dev/guide/examples/hello.html"));
    assert!(
        site["llms.txt"].contains("](https://oskalang.dev/guide/examples/hello.md)"),
        "{}",
        site["llms.txt"]
    );
}

// ---- The link checker ------------------------------------------------------------

/// Run the link checker on `site`, as `_site/`, with `tracked` as the
/// repository's tracked files.
fn check_links(site: &BTreeMap<String, String>, tracked: &[&str]) -> RunOutcome {
    let list: String = tracked.iter().map(|path| format!("{path}\n")).collect();
    let mut host = site
        .iter()
        .fold(FakeHost::new(), |host, (path, text)| {
            host.with_file(&format!("_site/{path}"), text)
        })
        .with_file("site/tracked.txt", &list);
    common::run_differential(&mut host, CHECK, &[])
}

/// The files the fixture site's links to GitHub name.
const FIXTURE_TRACKED: &[&str] = &["LICENSE", "README.md", "examples/shopping/src/main.osk"];

/// A small built site with every kind of link the checker follows: pages,
/// copies and anchors on the site, links to GitHub, and the outputs for
/// agents and crawlers.
fn fixture_site() -> BTreeMap<String, String> {
    site_of(&[
        (
            "README.md",
            "# Oska\n\nSee the [design](docs/design.md#values), the [LICENSE](LICENSE), \
             [the shop](examples/shopping) and [the web](https://example.com/x).\n",
        ),
        (
            "docs/design.md",
            "# Design\n\n## Values\n\nBack [home](../README.md), [up](#values), \
             [a decision](adr/0001-a.md#why).\n",
        ),
        ("docs/adr/0001-a.md", "# A\n\n## Why\n\nBecause.\n"),
        // The pages `llms.txt` always links.
        ("docs/guide/packages.md", "# Packages\n"),
        (
            "docs/guide/cli-reference.md",
            "# CLI\n\n## Platform support\n",
        ),
    ])
}

/// Assert that the checker refuses `site`, naming `file` and `url`.
fn assert_finds(site: &BTreeMap<String, String>, file: &str, url: &str) {
    let outcome = check_links(site, FIXTURE_TRACKED);
    assert_eq!(outcome.exit_code, 1, "the check should fail: {outcome:#?}");
    let line = format!("{file}: {url}: ");
    assert!(
        outcome.stderr.contains(&line),
        "the finding should start {line:?}: {}",
        outcome.stderr
    );
}

/// `site` with `extra` appended to the file at `path`.
fn with_appended(
    site: &BTreeMap<String, String>,
    path: &str,
    extra: &str,
) -> BTreeMap<String, String> {
    let mut site = site.clone();
    let text = site
        .get_mut(path)
        .unwrap_or_else(|| panic!("the site has no {path}"));
    text.push_str(extra);
    site
}

#[test]
fn the_checker_passes_a_site_whose_every_link_goes_somewhere() {
    let outcome = check_links(&fixture_site(), FIXTURE_TRACKED);
    assert!(succeeded(&outcome), "the check should pass: {outcome:#?}");
}

#[test]
fn the_checker_fails_on_a_link_to_a_page_the_site_lacks() {
    let site = with_appended(
        &fixture_site(),
        "design.html",
        "<p><a href=\"/nope.html\">x</a></p>\n",
    );
    assert_finds(&site, "design.html", "/nope.html");
    let site = with_appended(
        &fixture_site(),
        "adr/0001-a.html",
        "<p><a href=\"../gone/\">x</a></p>\n",
    );
    assert_finds(&site, "adr/0001-a.html", "../gone/");
}

#[test]
fn the_checker_fails_on_a_fragment_the_page_lacks() {
    let site = with_appended(
        &fixture_site(),
        "index.html",
        "<p><a href=\"/design.html#no-such-heading\">x</a></p>\n",
    );
    assert_finds(&site, "index.html", "/design.html#no-such-heading");
    let site = with_appended(
        &fixture_site(),
        "design.html",
        "<p><a href=\"#nowhere\">x</a></p>\n",
    );
    assert_finds(&site, "design.html", "#nowhere");
    let site = with_appended(
        &fixture_site(),
        "design.md",
        "\n[x](/adr/0001-a.md#nowhere)\n",
    );
    assert_finds(&site, "design.md", "/adr/0001-a.md#nowhere");
}

#[test]
fn the_checker_fails_on_a_github_link_to_a_file_the_repository_does_not_track() {
    let outcome = check_links(
        &fixture_site(),
        &["README.md", "examples/shopping/src/main.osk"],
    );
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    let url = format!("{BLOB}LICENSE");
    for file in ["index.html", "index.md", "llms-full.txt"] {
        assert!(
            outcome.stderr.contains(&format!("{file}: {url}: ")),
            "{file} should be named: {}",
            outcome.stderr
        );
    }
    // A directory the repository tracks a file in is there.
    assert!(
        !outcome.stderr.contains("examples/shopping: "),
        "{}",
        outcome.stderr
    );
}

#[test]
fn the_checker_fails_on_a_dead_link_in_a_copy_or_a_file_for_agents() {
    let site = with_appended(&fixture_site(), "design.md", "\nSee [gone](/gone.md).\n");
    assert_finds(&site, "design.md", "/gone.md");
    let site = with_appended(
        &fixture_site(),
        "llms.txt",
        "- [Gone](https://oskalang.dev/gone.md)\n",
    );
    assert_finds(&site, "llms.txt", "https://oskalang.dev/gone.md");
    let site = with_appended(&fixture_site(), "llms-full.txt", "\n[gone](../gone.md)\n");
    assert_finds(&site, "llms-full.txt", "../gone.md");
    let mut site = fixture_site();
    let sitemap = site["sitemap.xml"].replace(
        "</urlset>",
        "<url><loc>https://oskalang.dev/gone.html</loc></url>\n</urlset>",
    );
    site.insert("sitemap.xml".to_string(), sitemap);
    assert_finds(&site, "sitemap.xml", "https://oskalang.dev/gone.html");
}

#[test]
fn the_checker_ignores_links_in_code_and_urls_off_the_site() {
    let site = with_appended(
        &fixture_site(),
        "design.md",
        "\n`[x](/gone.md)` and\n\n```\n[y](/gone.md)\n```\n\n\
         [web](https://example.com/gone) [mail](mailto:a@b.c)\n",
    );
    let outcome = check_links(&site, FIXTURE_TRACKED);
    assert!(succeeded(&outcome), "the check should pass: {outcome:#?}");
}

#[test]
fn the_checker_fails_closed_without_the_tracked_files() {
    let mut host = fixture_site()
        .iter()
        .fold(FakeHost::new(), |host, (path, text)| {
            host.with_file(&format!("_site/{path}"), text)
        })
        .with_dir("site");
    // On the interpreter only: the two backends word a missing file's
    // error differently, and the exit code is what matters here.
    let args = [common::PROGRAM_PATH.to_string()];
    let outcome = oska_run::run(CHECK, &mut host, &args);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    assert!(outcome.stderr.contains("tracked.txt"), "{outcome:#?}");
}

#[test]
fn the_checker_reads_only_the_site_and_the_tracked_files() {
    let report = oska_check::check_module(CHECK).authority_to_human("site/check-links.osk");
    assert!(report.starts_with("fn main: FsRead (read)\n"), "{report}");
    for line in [
        "  FsRead -> DirRead: restrict_to \"_site\" (bytes ",
        "  FsRead -> FileRead: restrict_to_file \"site/tracked.txt\" (bytes ",
    ] {
        assert!(report.contains(line), "no {line:?} in {report}");
    }
    assert_eq!(report.matches("  FsRead -> ").count(), 2, "{report}");
}

// ---- The real site ---------------------------------------------------------------

/// The repository root.
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `dir`, by its path relative to `dir`.
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
    walk(dir, dir, &mut out);
    out
}

/// The real site, as the Pages workflow builds it: the generator on the
/// repository's own inputs, then the files `scripts/release/stage-site.sh`
/// adds, here as stand-ins. It is built once, for every test that reads it.
fn real_site() -> &'static BTreeMap<String, String> {
    static SITE: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    SITE.get_or_init(build_real_site)
}

fn build_real_site() -> BTreeMap<String, String> {
    let root = root();
    let mut inputs: Vec<(String, String)> = Vec::new();
    for dir in ["docs", "site/agent-setup"] {
        for (path, text) in read_tree(&root.join(dir)) {
            inputs.push((format!("{dir}/{path}"), text));
        }
    }
    for file in [
        "README.md",
        "SECURITY.md",
        "GLOSSARY.md",
        "Cargo.toml",
        "site/style.css",
        "site/search.js",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect("a repository file");
        inputs.push((file.to_string(), text));
    }
    let authority = oska_check::check_module(BUILD).authority_to_human("site/build.osk");
    inputs.push(("site/authority.txt".to_string(), authority));
    let refs: Vec<(&str, &str)> = inputs
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let mut site = site_of(&refs);
    for staged in ["install.sh", "install.ps1", "oska-release.pub"] {
        site.insert(staged.to_string(), "staged\n".to_string());
    }
    site
}

/// The repository's files, as `git ls-files` lists them, with files not
/// yet added (but not ignored), so a link to a new file checks before it
/// is committed. `None` if git cannot list them.
fn tracked_files() -> Option<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.lines().map(str::to_string).collect())
}

#[test]
fn every_link_in_the_real_site_goes_somewhere() {
    let Some(tracked) = tracked_files() else {
        eprintln!("skipped: git cannot list the repository's files");
        return;
    };
    let tracked: Vec<&str> = tracked.iter().map(String::as_str).collect();
    let outcome = check_links(real_site(), &tracked);
    assert!(
        succeeded(&outcome),
        "links in the real site go nowhere:\n{}",
        outcome.stderr
    );
}

#[test]
fn the_dead_targets_the_site_review_found_now_resolve() {
    let site = real_site();
    assert!(site.contains_key("guide/examples/hello.html"));
    assert!(site.contains_key("guide/examples/capabilities.md"));
    for link in [
        "[hello](/guide/examples/hello.md)",
        "[capabilities](/guide/examples/capabilities.md)",
    ] {
        assert!(
            site["guide/index.md"].contains(link),
            "no {link} in the guide index"
        );
    }
    for copy in site.keys().filter(|k| k.ends_with(".md")) {
        for dead in ["](LICENSE)", "](examples/shopping)", "](../../examples/"] {
            assert!(!site[copy].contains(dead), "{copy} still has {dead}");
        }
    }
}

#[test]
fn the_pages_workflow_checks_every_link_after_staging_and_before_upload() {
    let workflow = include_str!("../../../.github/workflows/pages.yml");
    let at = |needle: &str| {
        workflow
            .find(needle)
            .unwrap_or_else(|| panic!("pages.yml does not run `{needle}`"))
    };
    let staged = at("scripts/release/stage-site.sh");
    let listed = at("ls-files > site/tracked.txt");
    let checked = at("./target/release/oskac run site/check-links.osk");
    let uploaded = at("actions/upload-pages-artifact");
    assert!(staged < checked && listed < checked && checked < uploaded);
}
