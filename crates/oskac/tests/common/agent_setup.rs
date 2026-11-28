// SPDX-License-Identifier: Apache-2.0
//! Helpers shared by the agent-setup suites (`agent_setup.rs` and
//! `agent_setup_hook.rs`): the repository's files, the `oskac` binary, and the
//! files an agent reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::Json;
use oska_run::FakeHost;

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn setup_file(name: &str) -> String {
    read(&repo_root().join("site/agent-setup").join(name))
}

/// The files an agent reads and is told to run or copy, by name.
pub const AGENT_FILES: [&str; 4] = [
    "prompt.md",
    "AGENTS.template.md",
    "CLAUDE.template.md",
    "oska-check.sh",
];

/// The text of every file an agent reads: [`AGENT_FILES`], then every
/// file of the Claude Code plugin, each with its name.
pub fn agent_texts() -> Vec<(String, String)> {
    let mut texts: Vec<(String, String)> = AGENT_FILES
        .iter()
        .map(|name| (name.to_string(), setup_file(name)))
        .collect();
    for file in plugin_files() {
        let text = read(&repo_root().join(&file));
        texts.push((file, text));
    }
    texts
}

pub fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

pub fn stdout_of(args: &[&str]) -> String {
    let output = oskac(args);
    assert_eq!(output.status.code(), Some(0), "oskac {args:?}");
    String::from_utf8(output.stdout).expect("UTF-8")
}

/// The marketplace file at the repository root, from which
/// `/plugin marketplace add aparsons/oska` reads the plugin list.
pub const MARKETPLACE: &str = ".claude-plugin/marketplace.json";

/// The plugin's directory, from the repository root.
pub const PLUGIN_DIR: &str = "plugins/oska";

/// Every file of the plugin, as `/`-separated paths from the repository
/// root, sorted, with the marketplace file first.
pub fn plugin_files() -> Vec<String> {
    fn walk(root: &Path, relative: &str, out: &mut Vec<String>) {
        let mut names: Vec<_> = std::fs::read_dir(root.join(relative))
            .unwrap_or_else(|e| panic!("cannot list {relative}: {e}"))
            .map(|e| e.expect("a directory entry").file_name())
            .collect();
        names.sort();
        for name in names {
            let path = format!("{relative}/{}", name.to_string_lossy());
            if root.join(&path).is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = vec![MARKETPLACE.to_string()];
    walk(&repo_root(), PLUGIN_DIR, &mut files);
    files
}

/// The real generator, run on the real docs and setup files in a fake
/// filesystem: what it writes under `_site/`.
/// Built once per test program: a build takes seconds.
pub fn built_site() -> &'static BTreeMap<String, String> {
    static SITE: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    SITE.get_or_init(build_site)
}

/// [`built_site`], built anew.
pub fn build_site() -> BTreeMap<String, String> {
    fn add(host: FakeHost, root: &Path, relative: &str) -> FakeHost {
        let path = root.join(relative);
        if path.is_dir() {
            let mut host = host.with_dir(relative);
            let mut names: Vec<_> = std::fs::read_dir(&path)
                .expect("a readable directory")
                .map(|e| e.expect("a directory entry").file_name())
                .collect();
            names.sort();
            for name in names {
                host = add(
                    host,
                    root,
                    &format!("{relative}/{}", name.to_string_lossy()),
                );
            }
            host
        } else {
            host.with_file(relative, &read(&path))
        }
    }
    let root = repo_root();
    let mut host = FakeHost::new();
    for relative in [
        "docs",
        "README.md",
        "GLOSSARY.md",
        "site/style.css",
        "site/search.js",
        "site/favicon.svg",
        "site/agent-setup",
        "Cargo.toml",
    ] {
        host = add(host, &root, relative);
    }
    let build = read(&root.join("site/build.osk"));
    let outcome = oska_run::run(&build, &mut host, &["site/build.osk".to_string()]);
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
    host.files_under("_site")
}

pub fn string(json: Option<&Json>) -> &str {
    match json {
        Some(Json::Str(s)) => s,
        other => panic!("{other:?} is not a string"),
    }
}

/// The one entry of `event`, and the one hook in it.
pub fn only_hook<'j>(settings: &'j Json, event: &str) -> (&'j Json, &'j Json) {
    let Some(Json::Arr(entries)) = settings.get("hooks").and_then(|h| h.get(event)) else {
        panic!("a {event} list");
    };
    assert_eq!(entries.len(), 1, "{event}");
    let Some(Json::Arr(hooks)) = entries[0].get("hooks") else {
        panic!("a hooks list");
    };
    assert_eq!(hooks.len(), 1, "{event}");
    (&entries[0], &hooks[0])
}
