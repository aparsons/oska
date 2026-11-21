//! Running the site generator, `site/build.osk`, and the link checker,
//! `site/check-links.osk`, on a [`FakeHost`], and the repository's own
//! files as the generator reads them. The site tests share these.
//!
//! Every run is differential like any other helper here: on aarch64 and
//! x86_64 Linux the program also runs natively on the same files and must
//! do the same.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oska_run::{FakeHost, RunOutcome};

use super::run_differential;

/// The generator's source.
pub const BUILD: &str = include_str!("../../../../site/build.osk");

/// The link checker's source.
pub const CHECK: &str = include_str!("../../../../site/check-links.osk");

/// The Pages workflow, which builds, checks and deploys the site.
pub const PAGES_WORKFLOW: &str = include_str!("../../../../.github/workflows/pages.yml");

/// The Content Security Policy every page carries: nothing runs but the
/// site's own search script, which may read only the site's own files,
/// and styles come only from the site. No font is fetched: the site uses
/// the system's fonts, and `font-src` falls back to `default-src 'none'`.
pub const CSP: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
    style-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'";

/// The favicon the tests' repositories hold.
pub const FAVICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 8 8\"><rect width=\"8\" height=\"8\"/></svg>\n";

/// The files every repository the generator reads must have, as small as
/// they can be. A test's own inputs replace them.
pub const MINIMAL_REPO: &[(&str, &str)] = &[
    ("docs/index.md", "# Oska\n"),
    ("GLOSSARY.md", "# Glossary\n"),
    ("docs/design.md", "# Design\n"),
    ("site/style.css", "body {}\n"),
    ("site/search.js", "// search\n"),
    ("site/favicon.svg", FAVICON),
    (
        "Cargo.toml",
        "[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
    ),
];

/// The repository the agent outputs are tested on: a home page, a glossary,
/// two docs, and a section of two.
pub fn agent_repo() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "docs/index.md",
            "# Oska\n\nA language where authority is explicit.\n\nSee the [design](design.md).\n",
        ),
        ("GLOSSARY.md", "# Words\n\n**Capability**: a value.\n"),
        (
            "docs/design.md",
            "# Design\n\nHow Oska works, and why.\n\n## Values\n\nText.\n\n\
             [adr](adr/) [b](adr/0002-b.md#x) [up](index.md) [out](https://example.com/a?b=1) \
             [src](../crates/x.rs) [here](#values)\n\n```\n[a](adr/)\n```\n",
        ),
        (
            "docs/adr/0001-a.md",
            "# A decision\n\nWe chose A.\n\n[design](../design.md#values)\n",
        ),
        ("docs/adr/0002-b.md", "# Another\n\nWe chose B.\n"),
    ]
}

/// The repository root.
pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `dir`, by its path relative to `dir`, with `/`
/// between components. No files if `dir` does not exist.
pub fn read_tree(dir: &Path) -> BTreeMap<String, String> {
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

/// Run the generator on a repository holding `inputs`, paths relative to
/// its root, with no `_site/`: how it ended, and the files it wrote under
/// `_site/`, by path relative to it.
pub fn build(inputs: &BTreeMap<String, String>) -> (RunOutcome, BTreeMap<String, String>) {
    let mut host = inputs.iter().fold(FakeHost::new(), |host, (path, text)| {
        host.with_file(path, text)
    });
    let outcome = run_differential(&mut host, BUILD, &[]);
    (outcome, host.files_under("_site"))
}

/// [`build`] on the files of `base` with `inputs` added, or put in place
/// of the files of the same path.
pub fn generate(
    base: &[(&str, &str)],
    inputs: &[(&str, &str)],
) -> (RunOutcome, BTreeMap<String, String>) {
    let files: BTreeMap<String, String> = base
        .iter()
        .chain(inputs)
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    build(&files)
}

/// Whether a run ended cleanly: exit code 0, no diagnostic, no panic.
pub fn succeeded(outcome: &RunOutcome) -> bool {
    outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none()
}

pub fn assert_succeeded(outcome: &RunOutcome) {
    assert!(
        succeeded(outcome),
        "the generator should succeed: {outcome:#?}"
    );
}

/// The site the generator writes from `base` and `inputs` ([`generate`]),
/// asserting that it succeeded.
pub fn site_of(base: &[(&str, &str)], inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let (outcome, site) = generate(base, inputs);
    assert_succeeded(&outcome);
    site
}

/// [`site_of`] on [`MINIMAL_REPO`], asserting also that every page it
/// wrote is inert.
pub fn build_ok(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let site = site_of(MINIMAL_REPO, inputs);
    super::inert::assert_site_inert(&site);
    site
}

/// The text of `page` between the first `open` and the next `close`.
pub fn between<'a>(page: &'a str, open: &str, close: &str) -> &'a str {
    let start = page.find(open).map(|at| at + open.len());
    let start = start.unwrap_or_else(|| panic!("no {open} in {page}"));
    let len = page[start..]
        .find(close)
        .unwrap_or_else(|| panic!("no {close} after {open} in {page}"));
    &page[start..start + len]
}

/// The authority report `oskac check site/build.osk` prints for the
/// generator as it is now.
pub fn generator_authority() -> String {
    oska_check::check_module(BUILD).authority_to_human("site/build.osk")
}

/// The repository's own inputs to the generator, as it reads them, and
/// what the Pages workflow writes just before the build.
pub fn real_inputs() -> BTreeMap<String, String> {
    let root = repository_root();
    let mut inputs: BTreeMap<String, String> = read_tree(&root.join("docs"))
        .into_iter()
        .map(|(path, text)| (format!("docs/{path}"), text))
        .collect();
    for (path, text) in read_tree(&root.join("site/agent-setup")) {
        inputs.insert(format!("site/agent-setup/{path}"), text);
    }
    for file in [
        "SECURITY.md",
        "CHANGELOG.md",
        "GLOSSARY.md",
        "Cargo.toml",
        "site/style.css",
        "site/search.js",
        "site/favicon.svg",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect("a repository file");
        inputs.insert(file.to_string(), text);
    }
    inputs.insert("site/authority.txt".to_string(), generator_authority());
    // The Pages workflow writes the whole reference with `oskac doc
    // --format=json`, which this crate cannot run: these tests use the part
    // of it that `oskac`'s own tests hold to the command's output, the
    // `std.sql` module. `oskac`'s tests build the site from all of it.
    let api = std::fs::read_to_string(root.join("crates/oskac/tests/golden/doc_std_sql.json"))
        .expect("the std.sql reference golden file");
    inputs.insert("site/api.json".to_string(), api);
    inputs
}

/// The real site: the generator on [`real_inputs`], asserting that it
/// succeeded. It is built once per test binary and cloned for each
/// caller: a build takes seconds, and several tests read it.
pub fn real_site() -> BTreeMap<String, String> {
    static SITE: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    SITE.get_or_init(|| {
        let (outcome, site) = build(&real_inputs());
        assert_succeeded(&outcome);
        site
    })
    .clone()
}

/// The repository's tracked files, as the Pages workflow lists them:
/// plain `git ls-files`, so a file that is not committed or staged is not
/// tracked, as it is not on CI. Fails if git cannot list them.
pub fn tracked_files() -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("ls-files")
        .current_dir(repository_root())
        .output()
        .unwrap_or_else(|e| panic!("git ls-files should run: {e}"));
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracked: Vec<String> = String::from_utf8(output.stdout)
        .expect("git ls-files prints UTF-8")
        .lines()
        .map(str::to_string)
        .collect();
    assert!(
        !tracked.is_empty(),
        "git ls-files should list the repository"
    );
    tracked
}

/// Run the link checker on `site`, as `_site/`, with `tracked` as the
/// repository's tracked files.
pub fn check_links(site: &BTreeMap<String, String>, tracked: &[&str]) -> RunOutcome {
    let list: String = tracked.iter().map(|path| format!("{path}\n")).collect();
    let mut host = site
        .iter()
        .fold(FakeHost::new(), |host, (path, text)| {
            host.with_file(&format!("_site/{path}"), text)
        })
        .with_file("site/tracked.txt", &list);
    run_differential(&mut host, CHECK, &[])
}

// ---- Reading pages and the agent corpus ----------------------------------------------

/// Every `href` in `html`, in order.
pub fn hrefs(html: &str) -> Vec<&str> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// The links of a page's navigation, as `(href, label)`, in order.
pub fn navigation(page: &str) -> Vec<(String, String)> {
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

/// The text of the `## heading` group of `llms`, up to the next group.
pub fn llms_group<'a>(llms: &'a str, heading: &str) -> &'a str {
    let open = format!("\n## {heading}\n");
    let start = llms
        .find(&open)
        .unwrap_or_else(|| panic!("no group {heading} in:\n{llms}"))
        + open.len();
    let rest = &llms[start..];
    &rest[..rest.find("\n## ").unwrap_or(rest.len())]
}

/// The URL of every entry in `text`, in order, without the origin.
pub fn entry_urls(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with("- ["))
        .map(|l| {
            let rest = l.split("](https://oskalang.dev/").nth(1).expect("a URL");
            rest.split(')').next().expect("a close").to_string()
        })
        .collect()
}
