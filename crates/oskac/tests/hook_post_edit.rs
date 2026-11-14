//! `oskac hook claude --event=post-edit` (spec #185, ticket #194): after
//! Claude Code edits a `.osk` file or an `oska.toml`, check what it
//! belongs to and hand back only the diagnostics, compactly.
//!
//! Each test runs the real binary with Claude Code's input on standard
//! input, as the published settings do, and asserts what the agent sees:
//! the exit status, standard output and standard error.

mod common;

use std::path::{Path, PathBuf};

use common::Json;

use common::hook::{
    edit_of, hook, post_edit, scratch, snapshot, symlink_file, tool_call, write_all, HELLO,
    MANIFEST,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Three diagnostics, one of each applicability: a `machine` fix on line
/// 1, a `maybe` fix on line 10, and a `placeholders` fix on line 11.
const THREE_FIXES: &str = "\
struct Holder {
    out: Stdout,
}

enum Light {
    Red,
    Green,
}

fn name(light: Light, other: Lightt) -> Int {
    match light {
        Light.Red => 1
    }
}

fn main() {}
";

#[test]
fn the_hook_is_quiet_for_an_osk_file_that_checks() {
    let dir = scratch("hook-good");
    write_all(&dir, &[("ok.osk", HELLO)]);
    let hooked = post_edit(&edit_of(&dir.join("ok.osk"), &dir), &dir);
    assert_eq!(
        (hooked.code, hooked.stdout.as_str(), hooked.stderr.as_str()),
        (Some(0), "", "")
    );
}

/// `oskac hook claude --event=post-edit --format=json`.
fn post_edit_json(input: &str, cwd: &Path) -> common::hook::Hooked {
    hook(
        &["hook", "claude", "--event=post-edit", "--format=json"],
        input,
        cwd,
    )
}

fn text_of(json: Option<&Json>) -> &str {
    match json {
        Some(Json::Str(s)) => s,
        other => panic!("{other:?} is not a string"),
    }
}

#[test]
fn with_format_json_the_hook_hands_back_one_json_object_with_the_diagnostic_fields() {
    let dir = scratch("hook-json");
    write_all(&dir, &[("app.osk", THREE_FIXES), ("ok.osk", HELLO)]);
    let hooked = post_edit_json(&edit_of(&dir.join("app.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.stdout, "");
    assert_eq!(hooked.lines().len(), 1, "one line: {}", hooked.stderr);
    let message = Json::parse(&hooked.stderr);
    assert_eq!(text_of(message.get("target")), "app.osk");
    let Some(Json::Arr(diagnostics)) = message.get("diagnostics") else {
        panic!("a diagnostics list: {}", hooked.stderr);
    };
    let codes: Vec<&str> = diagnostics.iter().map(|d| text_of(d.get("code"))).collect();
    assert_eq!(codes, ["E0210", "E0200", "E0253"]);
    // The fields of #192, as `oskac check --format=json` gives them.
    let first = &diagnostics[0];
    assert_eq!(text_of(first.get("path")), "app.osk");
    assert_eq!(first.get("line"), Some(&Json::Num(1)));
    assert_eq!(first.get("column"), Some(&Json::Num(8)));
    assert_eq!(text_of(first.get("severity")), "error");
    assert_eq!(
        text_of(first.get("url")),
        "https://oskalang.dev/codes/E0210"
    );
    assert_eq!(
        text_of(first.get("fix").and_then(|f| f.get("applicability"))),
        "machine"
    );
    assert_eq!(message.get("more"), Some(&Json::Num(0)));
    assert_eq!(message.get("machine_fixes"), Some(&Json::Num(1)));
    assert_eq!(text_of(message.get("fix_command")), "oskac fix app.osk");
    // A clean file is as quiet as ever.
    let hooked = post_edit_json(&edit_of(&dir.join("ok.osk"), &dir), &dir);
    assert_eq!(
        (hooked.code, hooked.stdout.as_str(), hooked.stderr.as_str()),
        (Some(0), "", "")
    );
}

#[test]
fn with_format_json_the_cap_is_kept_and_no_fix_command_is_null() {
    let dir = scratch("hook-json-cap");
    let mut source = String::new();
    for n in 0..25 {
        source.push_str(&format!("fn f{n}() {{ let x: Int = \"a\" }}\n"));
    }
    write_all(&dir, &[("many.osk", &source)]);
    let hooked = post_edit_json(&edit_of(&dir.join("many.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let message = Json::parse(&hooked.stderr);
    let Some(Json::Arr(diagnostics)) = message.get("diagnostics") else {
        panic!("a diagnostics list: {}", hooked.stderr);
    };
    assert_eq!(diagnostics.len(), 20);
    assert_eq!(message.get("more"), Some(&Json::Num(5)));
    assert_eq!(message.get("machine_fixes"), Some(&Json::Num(0)));
    assert_eq!(message.get("fix_command"), Some(&Json::Null));
}

#[test]
fn the_hook_hands_back_only_the_diagnostics_with_line_and_column_and_exit_status_2() {
    let dir = scratch("hook-bad");
    write_all(&dir, &[("app.osk", THREE_FIXES)]);
    let hooked = post_edit(&edit_of(&dir.join("app.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.stdout, "", "the hook prints nothing on stdout");
    let lines = hooked.lines();
    assert_eq!(lines.len(), 4, "{}", hooked.stderr);
    // One line per diagnostic, in the order the checker reports them, with
    // the path written from the project's directory.
    assert!(
        lines[0].starts_with("app.osk:1:8: error[E0210]: "),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("app.osk:10:30: error[E0200]: "),
        "{lines:?}"
    );
    assert!(
        lines[2].starts_with("app.osk:11:11: error[E0253]: "),
        "{lines:?}"
    );
    // Then how many have a `machine` fix, and the command that applies them.
    assert_eq!(
        lines[3],
        "1 of 3 diagnostics have a `machine` fix: run `oskac fix app.osk`"
    );
    // Only the diagnostics: no authority report, no JSON, no fix lines.
    assert!(!hooked.stderr.contains("    fix"), "{}", hooked.stderr);
    assert!(!hooked.stderr.contains("bytes "), "{}", hooked.stderr);
    assert!(!hooked.stderr.contains('{'), "{}", hooked.stderr);
}

#[test]
fn the_hook_does_not_mention_oskac_fix_when_no_diagnostic_has_a_machine_fix() {
    let dir = scratch("hook-no-fix");
    write_all(&dir, &[("bad.osk", "fn main() { let x: Int = \"a\" }\n")]);
    let hooked = post_edit(&edit_of(&dir.join("bad.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 1, "{}", hooked.stderr);
    assert!(lines[0].starts_with("bad.osk:1:"), "{lines:?}");
    assert!(lines[0].contains("error[E0220]: "), "{lines:?}");
}

#[test]
fn the_hook_counts_only_the_fixes_oskac_fix_applies() {
    // Adding a bound, and qualifying `Db`, would widen authority, so they
    // are `maybe`: `oskac fix` leaves them alone, and the hook does not
    // send the agent to it for them.
    let dir = scratch("hook-widening-fixes");
    write_all(
        &dir,
        &[
            (
                "bound.osk",
                "fn same<T>(a: T, b: T) -> Bool {\n    a == b\n}\n\nfn main() {}\n",
            ),
            (
                "db.osk",
                "use std.sql\n\nfn query(db: Db) -> Int {\n    1\n}\n\nfn main() {}\n",
            ),
        ],
    );
    for file in ["bound.osk", "db.osk"] {
        let hooked = post_edit(&edit_of(&dir.join(file), &dir), &dir);
        assert_eq!(hooked.code, Some(2), "{file}: {}", hooked.stderr);
        assert!(
            !hooked.stderr.contains("oskac fix"),
            "{file}: {}",
            hooked.stderr
        );
        // And `oskac fix` agrees: it applies nothing.
        let fixed = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["fix", file, "--format=json"])
            .current_dir(&dir)
            .output()
            .expect("oskac should run");
        let out = String::from_utf8_lossy(&fixed.stdout);
        assert!(out.contains("\"applied\": []"), "{file}: {out}");
    }
}

#[test]
fn a_symlinked_file_is_refused_with_e0411_and_never_read() {
    let dir = scratch("hook-symlink");
    write_all(
        &dir,
        &[
            ("real/ok.osk", HELLO),
            ("app/oska.toml", MANIFEST),
            ("app/src/main.osk", HELLO),
            ("outside/util.osk", HELLO),
        ],
    );
    if !symlink_file(&dir.join("real/ok.osk"), &dir.join("link.osk"))
        || !symlink_file(&dir.join("outside/util.osk"), &dir.join("app/src/util.osk"))
    {
        return;
    }
    // A file alone, and a module of a package, the same way: what a link
    // points at is never the program's source.
    for (edited, shown) in [
        ("link.osk", "link.osk:1:1: error[E0411]: "),
        ("app/src/util.osk", "app/src/util.osk:1:1: error[E0411]: "),
    ] {
        let hooked = post_edit(&edit_of(&dir.join(edited), &dir), &dir);
        assert_eq!(hooked.code, Some(2), "{edited}: {}", hooked.stderr);
        assert!(
            hooked.lines().iter().any(|l| l.starts_with(shown)),
            "{edited}: {}",
            hooked.stderr
        );
    }
}

#[test]
fn the_hook_resolves_a_relative_path_against_the_projects_directory() {
    let dir = scratch("hook-relative");
    write_all(
        &dir,
        &[
            ("ok.osk", HELLO),
            ("sub/bad.osk", "fn main() { let x: Int = 1 + \"a\" }\n"),
        ],
    );
    let hooked = post_edit(&tool_call("ok.osk", &dir), &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // The hook runs where Claude Code runs it, but `cwd` is what it trusts.
    let elsewhere = scratch("hook-relative-elsewhere");
    let hooked = post_edit(&tool_call("sub/bad.osk", &dir), &elsewhere);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(hooked.stderr.contains("bad.osk:1:"), "{}", hooked.stderr);
}

#[test]
fn the_hook_checks_a_package_module_with_its_package() {
    // Checked alone, `main.osk` of a package reports `E0203` and `E0221`
    // for the modules it uses; with its package it has nothing to report.
    let main = repo_root().join("examples/shopping/src/main.osk");
    let dir = scratch("hook-package");
    let hooked = post_edit(&edit_of(&main, &dir), &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

/// A package `app` under `dir` whose `util` module has an error and whose
/// `main` module is fine.
fn broken_package(dir: &Path) {
    write_all(
        dir,
        &[
            ("app/oska.toml", MANIFEST),
            (
                "app/src/main.osk",
                "use pkg.util\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    io.println(util.name())\n}\n",
            ),
            (
                "app/src/util.osk",
                "pub(pkg) fn name() -> String {\n    let x: Int = \"a\"\n    \"n\"\n}\n",
            ),
        ],
    );
}

#[test]
fn the_hook_reports_an_error_in_another_module_of_the_package() {
    let dir = scratch("hook-package-bad");
    broken_package(&dir);
    let hooked = post_edit(&edit_of(&dir.join("app/src/main.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 1, "{}", hooked.stderr);
    assert!(
        lines[0].starts_with("app/src/util.osk:2:"),
        "the path is from the project's directory: {lines:?}"
    );
    assert!(lines[0].contains("error[E0220]: "), "{lines:?}");
    assert!(!hooked.stderr.contains("E0203"), "{}", hooked.stderr);
}

#[test]
fn the_hook_checks_the_package_of_an_oska_toml_edit() {
    let dir = scratch("hook-manifest");
    broken_package(&dir);
    let hooked = post_edit(&edit_of(&dir.join("app/oska.toml"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("app/src/util.osk:2:"),
        "{}",
        hooked.stderr
    );
    // A manifest of a package that checks has nothing to report.
    let good = scratch("hook-manifest-good");
    write_all(&good, &[("oska.toml", MANIFEST), ("src/main.osk", HELLO)]);
    let hooked = post_edit(&edit_of(&good.join("oska.toml"), &good), &good);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

#[test]
fn the_hook_ignores_a_file_that_is_not_osk_or_oska_toml_without_a_check() {
    let dir = scratch("hook-other");
    broken_package(&dir);
    write_all(
        &dir,
        &[
            ("app/notes.md", "text"),
            ("app/src/notes.md", "text"),
            ("app/src/oska.toml.bak", "text"),
        ],
    );
    // Each of these is in a package that does not check: a check would
    // end in status 2.
    for file in ["app/notes.md", "app/src/notes.md", "app/src/oska.toml.bak"] {
        let hooked = post_edit(&edit_of(&dir.join(file), &dir), &dir);
        assert_eq!(
            (hooked.code, hooked.stderr.as_str()),
            (Some(0), ""),
            "{file}"
        );
    }
}

#[test]
fn the_hook_shows_at_most_twenty_diagnostics_and_counts_the_rest() {
    let dir = scratch("hook-cap");
    let mut source = String::new();
    for n in 0..25 {
        source.push_str(&format!("struct Holder{n} {{\n    out: Stdout,\n}}\n\n"));
    }
    source.push_str("fn main() {}\n");
    write_all(&dir, &[("many.osk", &source)]);
    let hooked = post_edit(&edit_of(&dir.join("many.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 22, "{}", hooked.stderr);
    for (n, line) in lines[..20].iter().enumerate() {
        let expected = format!("many.osk:{}:8: error[E0210]: ", n * 4 + 1);
        assert!(line.starts_with(&expected), "{line}");
    }
    assert_eq!(lines[20], "and 5 more");
    assert_eq!(
        lines[21],
        "25 of 25 diagnostics have a `machine` fix: run `oskac fix many.osk`"
    );
}

#[test]
fn the_hook_names_the_package_to_fix_when_the_target_is_a_package() {
    let dir = scratch("hook-fix-package");
    write_all(
        &dir,
        &[
            ("app/oska.toml", MANIFEST),
            (
                "app/src/main.osk",
                "struct Holder {\n    out: Stdout,\n}\n\nfn main() {}\n",
            ),
        ],
    );
    let hooked = post_edit(&edit_of(&dir.join("app/src/main.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert!(
        lines[0].starts_with("app/src/main.osk:1:8: error[E0210]: "),
        "{lines:?}"
    );
    assert_eq!(
        lines[1],
        "1 of 1 diagnostics have a `machine` fix: run `oskac fix app`"
    );
}

#[test]
fn malformed_input_or_an_unreadable_file_is_one_line_and_exit_status_1() {
    let dir = scratch("hook-malformed");
    write_all(&dir, &[("ok.osk", HELLO)]);
    let missing = edit_of(&dir.join("missing.osk"), &dir);
    let inputs = [
        // Not JSON, and not even text.
        "this is not json",
        "",
        "{\"tool_input\":",
        "{} {}",
        // JSON of an unexpected shape: no file, or a file that is not a name.
        "{\"tool_input\":{}}",
        "{\"cwd\":\"/\"}",
        "{\"tool_input\":{\"file_path\":7}}",
        "[]",
        "null",
        // A file that is not there.
        missing.as_str(),
    ];
    for input in inputs {
        let hooked = post_edit(input, &dir);
        assert_eq!(hooked.code, Some(1), "{input}: {}", hooked.stderr);
        assert_eq!(hooked.stdout, "", "{input}");
        assert!(
            hooked.stderr.starts_with("error: "),
            "{input}: {}",
            hooked.stderr
        );
        assert_eq!(hooked.lines().len(), 1, "{input}: {}", hooked.stderr);
    }
}

#[test]
fn an_unknown_or_missing_event_is_one_line_and_exit_status_1() {
    let dir = scratch("hook-event");
    for args in [
        &["hook", "claude"][..],
        &["hook", "claude", "--event=sleep"],
        &["hook", "claude", "--event="],
    ] {
        let hooked = hook(args, "{}", &dir);
        assert_eq!(hooked.code, Some(1), "{args:?}: {}", hooked.stderr);
        assert!(hooked.stderr.starts_with("error: "), "{args:?}");
        assert_eq!(hooked.lines().len(), 1, "{args:?}: {}", hooked.stderr);
    }
}

#[test]
fn the_hook_writes_nothing() {
    let dir = scratch("hook-readonly");
    broken_package(&dir);
    write_all(&dir, &[("alone.osk", THREE_FIXES), ("ok.osk", HELLO)]);
    let before = snapshot(&dir);
    for file in ["app/src/main.osk", "app/oska.toml", "alone.osk", "ok.osk"] {
        post_edit(&edit_of(&dir.join(file), &dir), &dir);
    }
    assert_eq!(snapshot(&dir), before);
}

/// A path with a quote in it, which a regular expression over JSON cannot
/// read: the JSON writes it `a\"b.osk`. Windows has no such file names.
#[cfg(unix)]
#[test]
fn the_hook_reads_a_path_with_an_escaped_quote() {
    let dir = scratch("hook-quote");
    write_all(&dir, &[("a\"b.osk", "fn main() { let x: Int = \"a\" }\n")]);
    let hooked = post_edit(&edit_of(&dir.join("a\"b.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.starts_with("a\"b.osk:1:"),
        "{}",
        hooked.stderr
    );
}

/// The hook's source reads its input and the program's files and nothing
/// else: no file written, no program started, no network.
#[test]
fn the_hooks_source_writes_no_file_starts_no_program_and_opens_no_connection() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = vec![src.join("hook.rs")];
    if let Ok(entries) = std::fs::read_dir(src.join("hook")) {
        sources.extend(entries.map(|e| e.expect("entry").path()));
    }
    for file in sources {
        let text = read(&file);
        for word in [
            "std::net",
            "Command::new",
            "std::process::Command",
            "fs::write",
            "fs::create_dir",
            "fs::remove",
            "fs::rename",
            "fs::copy",
            "File::create",
            "OpenOptions",
        ] {
            assert!(!text.contains(word), "{} contains `{word}`", file.display());
        }
    }
}
