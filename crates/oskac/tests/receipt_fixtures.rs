//! `oskac self uninstall` against the shared install-receipt fixtures in
//! `scripts/install/receipt-fixtures` (issue #325). `scripts/install/test.sh`
//! and `test.ps1` run the same fixtures through the installers'
//! `--uninstall`, so the three cannot drift apart. The fixture format and the
//! rules they check are in `docs/guide/install-receipt.md`.
//!
//! `oskac` computes no SHA-256, so the `@SHA:name@` placeholders are filled
//! with a fixed digest here: what `oskac` checks instead is in the rules.

mod common;

use common::portable::{json_escape, output_of, set_home, shown, Scratch};
use common::Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The implementation these tests check, as the fixtures name it.
const IMPLEMENTATION: &str = if cfg!(windows) {
    "oskac_windows"
} else {
    "oskac_unix"
};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install/receipt-fixtures")
}

fn repo_file(path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The places a fixture's placeholders stand for.
struct Places {
    values: Vec<(&'static str, String)>,
}

impl Places {
    /// The places under `root`. `oska_home`, if given, is where the Oska
    /// home is, as a template on `@HOME@`; it is `@HOME@/.oska` otherwise.
    fn new(root: &Path, oska_home: Option<&str>) -> Places {
        let home = root.join("home");
        let oska = match oska_home {
            Some(template) => PathBuf::from(
                template
                    .replace('/', std::path::MAIN_SEPARATOR_STR)
                    .replace("@HOME@", &home.to_string_lossy()),
            ),
            None => home.join(".oska"),
        };
        let bin = oska.join("bin");
        let exe = bin.join(if cfg!(windows) { "oskac.exe" } else { "oskac" });
        let text = |p: &Path| p.to_string_lossy().into_owned();
        Places {
            // @OSKA_HOME@ before @HOME@ is not needed (the names differ), but
            // the longest first keeps it obviously right.
            values: vec![
                ("@OSKA_HOME@", text(&oska)),
                ("@PROJECT@", text(&home.join("project"))),
                ("@HOME@", text(&home)),
                ("@BIN@", text(&bin)),
                ("@EXE@", text(&exe)),
                ("@VERSION@", VERSION.to_string()),
            ],
        }
    }

    fn get(&self, name: &str) -> PathBuf {
        let value = self
            .values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .expect("a placeholder");
        PathBuf::from(value)
    }

    /// A path template: `/` becomes the platform's separator, then the
    /// placeholders their values.
    fn path(&self, template: &str) -> String {
        let mut text = template.replace('/', std::path::MAIN_SEPARATOR_STR);
        for (name, value) in &self.values {
            text = text.replace(name, value);
        }
        text
    }

    /// File contents: placeholders only, separators as written.
    fn text(&self, template: &str) -> String {
        let mut text = template.to_string();
        for (name, value) in &self.values {
            text = text.replace(name, value);
        }
        text
    }

    /// The receipt template: `/` becomes the platform's separator (JSON
    /// escaped), the placeholders their JSON-escaped values, and every
    /// `@SHA:name@` a fixed digest.
    fn receipt(&self, template: &str) -> String {
        let mut text = template.replace('/', &json_escape(std::path::MAIN_SEPARATOR_STR));
        for (name, value) in &self.values {
            text = text.replace(name, &json_escape(value));
        }
        while let Some(at) = text.find("@SHA:") {
            let end = at + 5 + text[at + 5..].find('@').expect("a closing @");
            text.replace_range(at..=end, &"0".repeat(64));
        }
        text
    }
}

fn string(value: &Json) -> &str {
    match value {
        Json::Str(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn strings(value: Option<&Json>) -> Vec<String> {
    match value {
        Some(Json::Arr(items)) => items.iter().map(|i| string(i).to_string()).collect(),
        None => Vec::new(),
        other => panic!("expected an array, got {other:?}"),
    }
}

fn items(value: Option<&Json>) -> &[Json] {
    match value {
        Some(Json::Arr(items)) => items,
        None => &[],
        other => panic!("expected an array, got {other:?}"),
    }
}

/// Every path under `root`, with what is there: a file's bytes, a link's
/// target, or a directory.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).expect("metadata");
            let what = if meta.file_type().is_symlink() {
                format!("link to {:?}", std::fs::read_link(&path).ok())
            } else if meta.is_dir() {
                stack.push(path.clone());
                "dir".to_string()
            } else {
                format!("{:?}", std::fs::read(&path).ok())
            };
            out.insert(path, what);
        }
    }
    out
}

/// A fixture's symlink, owned by an ordinary user. Uninstall trusts a
/// symlink root owns (it is the system's), so when the tests run as root
/// the link is handed to `nobody` (65534): it must still be refused.
#[cfg(unix)]
fn link(target: &Path, link: &Path) {
    use std::os::unix::fs::MetadataExt;
    std::os::unix::fs::symlink(target, link).expect("symlink");
    let meta = std::fs::symlink_metadata(link).expect("symlink metadata");
    if meta.uid() == 0 {
        std::os::unix::fs::lchown(link, Some(65534), Some(65534)).expect("lchown the symlink");
    }
}

#[cfg(windows)]
fn link(target: &Path, link: &Path) {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link).expect("symlink");
    } else {
        std::os::windows::fs::symlink_file(target, link).expect("symlink");
    }
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("dir");
    }
    std::fs::write(path, bytes).expect("write");
}

/// Set a fixture up, run the uninstall, and check the outcome. Returns
/// what was wrong, if anything.
fn run_case(name: &str, dir: &Path) -> Result<(), String> {
    let case = Json::parse(
        &String::from_utf8(std::fs::read(dir.join("case.json")).expect("case.json"))
            .expect("UTF-8"),
    );
    let Some(expect) = case.get("expect").and_then(|e| e.get(IMPLEMENTATION)) else {
        // The case does not apply on this platform.
        return Ok(());
    };
    let template = std::fs::read_to_string(dir.join("receipt.json")).expect("receipt.json");
    let scratch = Scratch::new("oskac-receipt", name);
    let root = PathBuf::from(shown(scratch.path()));
    let places = Places::new(&root, case.get("oska_home").map(string));
    let exe = places.get("@EXE@");
    let oska = places.get("@OSKA_HOME@");
    let home = places.get("@HOME@");

    // Links in place of a directory the rest is then written through.
    for item in items(case.get("link_first")) {
        let path = PathBuf::from(places.path(string(item.get("path").expect("path"))));
        let target = PathBuf::from(places.path(string(item.get("link_to").expect("link_to"))));
        std::fs::create_dir_all(&target).expect("link target");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        link(&target, &path);
    }
    std::fs::create_dir_all(places.get("@BIN@")).expect("bin");
    std::fs::copy(env!("CARGO_BIN_EXE_oskac"), &exe).expect("copy oskac");
    write(&oska.join("version"), format!("{VERSION}\n").as_bytes());
    for item in items(case.get("setup")) {
        let path = PathBuf::from(places.path(string(item.get("path").expect("path"))));
        if let Some(text) = item.get("text") {
            write(&path, places.text(string(text)).as_bytes());
        } else if let Some(which) = item.get("published") {
            let file = match string(which) {
                "hook_script" => "site/agent-setup/oska-check.sh",
                "hook_settings" => "site/agent-setup/claude-settings-post-edit.json",
                other => panic!("no published file {other}"),
            };
            write(&path, &repo_file(file));
        } else if let Some(target) = item.get("link_to") {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            link(Path::new(&places.path(string(target))), &path);
        } else {
            panic!("{name}: a setup entry needs text, published or link_to");
        }
    }
    write(
        &oska.join("receipt.json"),
        places.receipt(&template).as_bytes(),
    );
    for item in items(case.get("change")) {
        let path = places.path(string(item.get("path").expect("path")));
        write(
            Path::new(&path),
            places
                .text(string(item.get("text").expect("text")))
                .as_bytes(),
        );
    }

    let before = snapshot(&root);
    let mut command = Command::new(&exe);
    command
        .args(["self", "uninstall", "--format=json"])
        .env("OSKA_HOME", &oska)
        .current_dir(&root);
    set_home(&mut command, &home);
    let output = output_of(&mut command);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let said = || {
        format!(
            "exit {:?}, stdout {stdout}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    let Ok(result) = std::panic::catch_unwind(|| Json::parse(stdout.trim())) else {
        return Err(format!("not one JSON object: {}", said()));
    };

    if let Some(code) = expect.get("refused") {
        if output.status.code() != Some(1) || result.get("error") != Some(code) {
            return Err(format!("expected a refusal with {code:?}: {}", said()));
        }
        if let Some(reason) = expect.get("reason") {
            let message = result.get("message").map(string).unwrap_or_default();
            if !message.contains(string(reason)) {
                return Err(format!("expected a refusal saying {reason:?}: {}", said()));
            }
        }
        if snapshot(&root) != before {
            return Err("refused, but changed files".to_string());
        }
        return Ok(());
    }

    if output.status.code() != Some(0) || result.get("ok") != Some(&Json::Bool(true)) {
        return Err(format!("expected an uninstall: {}", said()));
    }
    // A running oskac.exe that Windows would not delete is moved aside and
    // listed as pending: it is gone from its place, as if removed.
    let mut removed = strings(result.get("removed"));
    for pending in items(result.get("pending")) {
        removed.push(string(pending.get("path").expect("path")).to_string());
        let _ = std::fs::remove_file(string(pending.get("moved_to").expect("moved_to")));
    }
    let mut kept = strings(result.get("kept"));
    let wanted = |key: &str| {
        let mut list: Vec<String> = strings(expect.get(key))
            .iter()
            .map(|p| places.path(p))
            .collect();
        list.sort();
        list
    };
    removed.sort();
    kept.sort();
    let mut problems = Vec::new();
    if removed != wanted("removed") {
        problems.push(format!(
            "removed {removed:?}, expected {:?}",
            wanted("removed")
        ));
    }
    if kept != wanted("kept") {
        problems.push(format!("kept {kept:?}, expected {:?}", wanted("kept")));
    }
    let hint = !matches!(result.get("path_hint"), Some(Json::Null) | None);
    if Some(&Json::Bool(hint)) != expect.get("path_hint") {
        problems.push(format!("path_hint is {:?}", result.get("path_hint")));
    }
    for path in &removed {
        if Path::new(path).exists() {
            problems.push(format!("{path} is reported removed and is still there"));
        }
    }
    for (path, _) in before {
        let listed = path.to_string_lossy().into_owned();
        if !removed.contains(&listed) && std::fs::symlink_metadata(&path).is_err() {
            problems.push(format!("{listed} is gone but not reported removed"));
        }
    }
    if let Some(Json::Obj(contents)) = expect.get("contents") {
        for (path, text) in contents {
            let path = places.path(path);
            let got = std::fs::read_to_string(&path).unwrap_or_default();
            if got != places.text(string(text)) {
                problems.push(format!("{path} holds {got:?}"));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("{}; {}", problems.join("; "), said()))
    }
}

fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
        .expect("the fixtures directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.len() > 20, "only {} fixtures", names.len());
    names
}

/// The kinds the receipt doc's "Entry kinds" table defines.
fn documented_kinds() -> Vec<String> {
    let doc = String::from_utf8(repo_file("docs/guide/install-receipt.md")).expect("UTF-8");
    let table = doc
        .split("## Entry kinds")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("an Entry kinds section")
        .to_string();
    let mut kinds: Vec<String> = table
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|rest| rest.split('`').next())
        .map(str::to_string)
        .collect();
    kinds.sort();
    kinds
}

/// Every documented kind has a fixture, every fixture's kinds are
/// documented (but for the one that shows an unknown kind is refused), and
/// every fixture says what each of the four implementations must do.
#[test]
fn the_fixtures_cover_every_documented_kind_and_every_implementation() {
    let mut used = std::collections::BTreeSet::new();
    for name in fixture_names() {
        let dir = fixtures_dir().join(&name);
        // A receipt this test's JSON reader does not take (a schema of
        // 1.0) is about how a receipt is read, not about a kind.
        let receipt = std::fs::read_to_string(dir.join("receipt.json")).expect("receipt.json");
        if let Ok(receipt) = std::panic::catch_unwind(|| Json::parse(&receipt)) {
            for file in items(receipt.get("files")) {
                if let Some(Json::Str(kind)) = file.get("kind") {
                    used.insert(kind.clone());
                }
            }
        }
        let case = Json::parse(&std::fs::read_to_string(dir.join("case.json")).expect("case.json"));
        let Some(Json::Obj(expect)) = case.get("expect") else {
            panic!("{name}: no expect object")
        };
        let mut keys: Vec<&str> = expect.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            ["oskac_unix", "oskac_windows", "ps1", "sh"],
            "{name}: expect needs an outcome for each implementation"
        );
    }
    assert!(used.remove("directory"), "the unknown-kind fixture is gone");
    let used: Vec<String> = used.into_iter().collect();
    assert_eq!(used, documented_kinds());
}

#[test]
fn self_uninstall_meets_every_receipt_fixture() {
    let names = fixture_names();
    let failures: Vec<String> = names
        .iter()
        .filter_map(|name| {
            run_case(name, &fixtures_dir().join(name))
                .err()
                .map(|why| format!("{name}: {why}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed:\n{}",
        failures.len(),
        names.len(),
        failures.join("\n")
    );
}
