//! The diagnostic JSON's additive fields (spec #185, ticket #192): every
//! diagnostic object, from every command, gains `line`, `column`,
//! `end_line` and `end_column`, `severity`, `url`, `related` and
//! `suggestions`, and every `fix` gains `applicability`. The fields that
//! were there before keep their names, types and meaning.

mod common;

use common::Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh directory for one test's files.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("diagnostic_json_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the scratch directory");
    dir
}

/// `files`, each a path relative to `dir` and its text, written there.
fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(file, text).expect("should write the file");
    }
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

/// The diagnostics of `oskac check <path> --format=json`, run in `dir`.
fn checked(dir: &Path, path: &str, extra: &[&str]) -> Vec<Json> {
    let mut args = vec!["check", path, "--format=json"];
    args.extend_from_slice(extra);
    let output = oskac(dir, &args);
    let report = Json::parse(&text(&output.stdout));
    match report.get("diagnostics") {
        Some(Json::Arr(diagnostics)) => diagnostics.clone(),
        other => panic!("no diagnostics in the report: {other:?}"),
    }
}

/// The one diagnostic with `code` among `diagnostics`.
fn with_code<'d>(diagnostics: &'d [Json], code: &str) -> &'d Json {
    let found: Vec<&Json> = diagnostics
        .iter()
        .filter(|d| d.get("code") == Some(&Json::Str(code.to_string())))
        .collect();
    assert_eq!(found.len(), 1, "one {code} in {diagnostics:?}");
    found[0]
}

fn num(n: u64) -> Json {
    Json::Num(n)
}

fn string(s: &str) -> Json {
    Json::Str(s.to_string())
}

#[test]
fn a_diagnostic_has_the_line_and_column_its_span_starts_and_ends_at_in_characters() {
    let dir = scratch("position");
    write(
        &dir,
        &[(
            "app.osk",
            "// ünïcode before the error\nfn main() {\n    let t = \"ééé\" + nme\n}\n",
        )],
    );
    let diagnostics = checked(&dir, "app.osk", &[]);
    let d = with_code(&diagnostics, "E0221");
    assert_eq!(d.get("line"), Some(&num(3)), "{d:?}");
    assert_eq!(d.get("column"), Some(&num(21)), "{d:?}");
    assert_eq!(d.get("end_line"), Some(&num(3)), "{d:?}");
    assert_eq!(d.get("end_column"), Some(&num(24)), "{d:?}");
    // The span stays in bytes: `ééé` is three characters and six bytes.
    let span = d.get("span").expect("a span");
    assert_eq!(span.get("start"), Some(&num(65)), "{d:?}");
    assert_eq!(span.get("end"), Some(&num(68)), "{d:?}");
}

#[test]
fn a_diagnostic_has_a_severity_a_url_and_lists_of_related_locations_and_suggestions() {
    let dir = scratch("severity");
    write(&dir, &[("app.osk", "fn main() {\n    let t = nme\n}\n")]);
    let diagnostics = checked(&dir, "app.osk", &[]);
    let d = with_code(&diagnostics, "E0221");
    assert_eq!(d.get("severity"), Some(&string("error")), "{d:?}");
    assert_eq!(
        d.get("url"),
        Some(&string("https://oskalang.dev/codes/E0221")),
        "{d:?}"
    );
    assert_eq!(d.get("related"), Some(&Json::Arr(Vec::new())), "{d:?}");
    assert_eq!(d.get("suggestions"), Some(&Json::Arr(Vec::new())), "{d:?}");
}

/// The applicability of the fix of the one diagnostic with `code`.
fn applicability<'d>(diagnostics: &'d [Json], code: &str) -> Option<&'d Json> {
    let d = with_code(diagnostics, code);
    d.get("fix")
        .unwrap_or_else(|| panic!("{code} should have a fix: {d:?}"))
        .get("applicability")
}

/// A program with a closest-name fix (`E0200`), a certain one (`E0210`)
/// and one that leaves `todo()` to fill in (`E0253`).
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
fn every_fix_says_how_safe_it_is_to_apply() {
    let dir = scratch("applicability");
    write(&dir, &[("app.osk", THREE_FIXES)]);
    let diagnostics = checked(&dir, "app.osk", &[]);
    assert_eq!(applicability(&diagnostics, "E0200"), Some(&string("maybe")));
    assert_eq!(
        applicability(&diagnostics, "E0210"),
        Some(&string("machine"))
    );
    assert_eq!(
        applicability(&diagnostics, "E0253"),
        Some(&string("placeholders"))
    );
}

#[test]
fn the_human_fix_line_says_when_a_fix_is_not_certain() {
    let dir = scratch("applicability_human");
    write(&dir, &[("app.osk", THREE_FIXES)]);
    let output = oskac(&dir, &["check", "app.osk"]);
    let stderr = text(&output.stderr);
    let fix_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("fix"))
        .collect();
    assert_eq!(fix_lines.len(), 3, "{stderr}");
    assert!(
        fix_lines
            .iter()
            .any(|l| l.starts_with("    fix (maybe): replace with `Light`")),
        "{stderr}"
    );
    assert!(
        fix_lines
            .iter()
            .any(|l| l.starts_with("    fix (placeholders): ")),
        "{stderr}"
    );
    assert!(
        fix_lines.iter().any(|l| l.starts_with("    fix: ")),
        "a machine fix keeps its line: {stderr}"
    );
}

/// The one JSON object a command printed on `stream`.
fn object(stream: &[u8]) -> Json {
    let text = text(stream);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one object: {text}");
    Json::parse(lines[0])
}

#[test]
fn lex_and_parse_errors_name_their_file_line_and_column() {
    let dir = scratch("lex_parse");
    write(
        &dir,
        &[
            ("lexed.osk", "fn main() {\n    let x = \"é\" $\n}\n"),
            ("parsed.osk", "fn main() {\n    let x =\n}\n"),
        ],
    );
    let lexed = object(&oskac(&dir, &["lex", "lexed.osk", "--format=json"]).stdout);
    assert_eq!(lexed.get("code"), Some(&string("E0001")), "{lexed:?}");
    assert_eq!(lexed.get("path"), Some(&string("lexed.osk")), "{lexed:?}");
    assert_eq!(lexed.get("line"), Some(&num(2)), "{lexed:?}");
    assert_eq!(lexed.get("column"), Some(&num(17)), "{lexed:?}");
    assert_eq!(lexed.get("end_column"), Some(&num(18)), "{lexed:?}");
    let parsed = object(&oskac(&dir, &["parse", "parsed.osk", "--format=json"]).stdout);
    assert_eq!(
        parsed.get("path"),
        Some(&string("parsed.osk")),
        "{parsed:?}"
    );
    assert!(parsed.get("line").is_some(), "{parsed:?}");
    assert!(parsed.get("end_column").is_some(), "{parsed:?}");
}

// ---- Every command ---------------------------------------------------------

/// Every diagnostic object in `value`: an object with a `code` and a
/// `span`, at any depth.
fn diagnostics_in(value: &Json, found: &mut Vec<Json>) {
    match value {
        Json::Obj(fields) => {
            if value.get("code").is_some() && value.get("span").is_some() {
                found.push(value.clone());
            }
            for (_, field) in fields {
                diagnostics_in(field, found);
            }
        }
        Json::Arr(items) => items.iter().for_each(|item| diagnostics_in(item, found)),
        _ => {}
    }
}

/// Every diagnostic object a command printed, on stdout or stderr: the
/// check report, which spans lines, or one JSON value per line. Lines
/// that are not JSON, the program's own output, are skipped.
fn printed_diagnostics(output: &Output) -> Vec<Json> {
    let mut found = Vec::new();
    for stream in [&output.stdout, &output.stderr] {
        let text = text(stream);
        if text.starts_with("{\n") {
            diagnostics_in(&Json::parse(&text), &mut found);
            continue;
        }
        for line in text.lines() {
            if line.starts_with('{') {
                diagnostics_in(&Json::parse(line), &mut found);
            }
        }
    }
    found
}

/// A program that panics with `P0004`, and a test of it that fails.
const PANICS: &str = "\
fn divide(a: Int, b: Int) -> Int {
    a / b
}

fn main() {
    let x = divide(1, 0)
}

test \"divides\" {
    t.assert_eq(divide(1, 0), 0)
}
";

/// A program the native backend cannot compile yet (`E0390`), which
/// `oskac run` hands to the interpreter with a note.
const NOT_NATIVE: &str = "\
fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let docs = fs.restrict_to(\"docs\")?
    dbg(docs.read_only())
    Ok(())
}
";

#[test]
fn every_command_gives_each_diagnostic_its_place_severity_and_lists() {
    let dir = scratch("every_command");
    write(
        &dir,
        &[
            ("panics.osk", PANICS),
            ("not_native.osk", NOT_NATIVE),
            ("broken.osk", "fn main() {\n    let t = nme\n}\n"),
        ],
    );
    let runs: [&[&str]; 7] = [
        &["check", "broken.osk", "--format=json"],
        &["run", "broken.osk", "--format=json"],
        &["run", "panics.osk", "--format=json"],
        &["run", "panics.osk", "--format=json", "--interpret"],
        &["test", "panics.osk", "--format=json"],
        &["test", "panics.osk", "--format=json", "--interpret"],
        &["build", "broken.osk", "--format=json"],
    ];
    for args in runs {
        let found = printed_diagnostics(&oskac(&dir, args));
        assert!(!found.is_empty(), "oskac {args:?} printed no diagnostic");
        for d in found {
            for key in [
                "line",
                "column",
                "end_line",
                "end_column",
                "severity",
                "url",
                "related",
                "suggestions",
            ] {
                assert!(d.get(key).is_some(), "oskac {args:?}: no `{key}` in {d:?}");
            }
        }
    }
}

#[test]
fn the_native_fallback_note_is_a_note_and_says_so_once() {
    let dir = scratch("note");
    write(&dir, &[("not_native.osk", NOT_NATIVE), ("docs/.keep", "")]);
    let output = oskac(&dir, &["run", "not_native.osk", "--format=json"]);
    let stderr = text(&output.stderr);
    let Some(line) = stderr.lines().find(|l| l.contains("\"E0390\"")) else {
        // A host without a native backend runs the program on the
        // interpreter without a note.
        return;
    };
    assert_eq!(line.matches("\"severity\"").count(), 1, "{line}");
    let note = Json::parse(line);
    assert_eq!(note.get("severity"), Some(&string("note")), "{line}");
    assert_eq!(note.get("line"), Some(&num(3)), "{line}");
}

// ---- Related locations -----------------------------------------------------

/// The related locations of `d`, each as (message, path, line, column).
fn related(d: &Json) -> Vec<(String, String, u64, u64)> {
    let Some(Json::Arr(all)) = d.get("related") else {
        panic!("no related list: {d:?}");
    };
    all.iter()
        .map(|r| {
            let field = |key: &str| match r.get(key) {
                Some(Json::Str(s)) => s.clone(),
                other => panic!("{key} should be a string: {other:?}"),
            };
            let number = |key: &str| match r.get(key) {
                Some(Json::Num(n)) => *n,
                other => panic!("{key} should be a number: {other:?}"),
            };
            assert!(r.get("span").is_some(), "a span: {r:?}");
            (
                field("message"),
                field("path"),
                number("line"),
                number("column"),
            )
        })
        .collect()
}

#[test]
fn a_name_defined_twice_points_at_its_first_definition() {
    let dir = scratch("related_duplicate");
    write(
        &dir,
        &[(
            "app.osk",
            "fn helper() {}\n\nstruct Point { x: Int }\n\nfn helper() {}\n\nenum Point { A }\n\nfn main() {}\n",
        )],
    );
    let diagnostics = checked(&dir, "app.osk", &[]);
    let twice: Vec<&Json> = diagnostics
        .iter()
        .filter(|d| d.get("code") == Some(&string("E0202")))
        .collect();
    assert_eq!(twice.len(), 2, "{diagnostics:?}");
    let first_function = (
        "first defined here".to_string(),
        "app.osk".to_string(),
        1,
        4,
    );
    let first_struct = (
        "first defined here".to_string(),
        "app.osk".to_string(),
        3,
        8,
    );
    assert_eq!(related(twice[0]), vec![first_function], "{:?}", twice[0]);
    assert_eq!(related(twice[1]), vec![first_struct], "{:?}", twice[1]);
}

#[test]
fn a_name_defined_twice_in_the_prelude_has_no_related_location() {
    let dir = scratch("related_prelude");
    write(&dir, &[("app.osk", "fn todo() {}\n\nfn main() {}\n")]);
    let diagnostics = checked(&dir, "app.osk", &[]);
    assert_eq!(related(with_code(&diagnostics, "E0202")), Vec::new());
}

#[test]
fn a_name_an_imported_module_lacks_points_at_that_module() {
    let dir = scratch("related_import");
    write(
        &dir,
        &[
            (
                "oska.toml",
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            ("src/util.osk", "// Helpers.\n\npub(pkg) fn shout() {}\n"),
            ("src/main.osk", "use pkg.util.{whisper}\n\nfn main() {}\n"),
        ],
    );
    let diagnostics = checked(&dir, ".", &[]);
    let d = with_code(&diagnostics, "E0205");
    assert_eq!(d.get("path"), Some(&string("src/main.osk")), "{d:?}");
    assert_eq!(
        related(d),
        vec![(
            "the module `pkg.util` it looked in".to_string(),
            "src/util.osk".to_string(),
            1,
            1
        )],
        "{d:?}"
    );
}

#[test]
fn an_impl_method_that_breaks_its_trait_points_at_the_trait_declaration() {
    let dir = scratch("related_trait");
    write(
        &dir,
        &[(
            "app.osk",
            "\
trait Shape {
    fn area(self) -> Int
        requires true
}

struct Square { side: Int }

impl Shape for Square {
    fn area(self) -> String {
        \"\"
    }
}

struct Circle { r: Int }

impl Shape for Circle {
    fn area(self) -> Int
        requires self.r > 0
    {
        1
    }
    fn perimeter(self) -> Int {
        2
    }
}

fn main() {}
",
        )],
    );
    let diagnostics = checked(&dir, "app.osk", &[]);
    let declared = ("declared here".to_string(), "app.osk".to_string(), 2, 8);
    let signature = with_code(&diagnostics, "E0262");
    assert_eq!(related(signature), vec![declared.clone()], "{signature:?}");
    let contract = with_code(&diagnostics, "E0218");
    assert_eq!(related(contract), vec![declared], "{contract:?}");
    let not_in_trait = with_code(&diagnostics, "E0263");
    assert_eq!(
        related(not_in_trait),
        vec![(
            "the trait `Shape` is declared here".to_string(),
            "app.osk".to_string(),
            1,
            7
        )],
        "{not_in_trait:?}"
    );
}

#[test]
fn a_manifest_entry_written_twice_points_at_the_first() {
    let dir = scratch("related_manifest");
    write(
        &dir,
        &[
            (
                "oska.toml",
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nname = \"again\"\n",
            ),
            ("src/main.osk", "fn main() {}\n"),
        ],
    );
    let diagnostics = checked(&dir, ".", &[]);
    let d = with_code(&diagnostics, "E0406");
    assert_eq!(
        related(d),
        vec![(
            "first written here".to_string(),
            "oska.toml".to_string(),
            2,
            1
        )],
        "{d:?}"
    );
}

// ---- The catalog -----------------------------------------------------------

/// Every code the checker's catalog, `docs/checker.md`, has a row for.
fn checker_codes() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc = std::fs::read_to_string(root.join("docs/checker.md")).expect("the checker notes");
    doc.lines()
        .filter_map(|line| {
            let code = line.strip_prefix("| `")?.split('`').next()?;
            (code.len() == 5 && code.starts_with('E')).then(|| code.to_string())
        })
        .collect()
}

/// `oskac explain <code> --format=json`.
fn explained(code: &str) -> Json {
    let output = oskac(Path::new("."), &["explain", code, "--format=json"]);
    assert!(output.status.success(), "explain {code}");
    object(&output.stdout)
}

/// Write the example program `body` into `dir`: a package when it is
/// written as files, each after a `==> path <==` line, or `app.osk`.
/// Returns the path to check.
fn write_example(dir: &Path, body: &str) -> String {
    if !body.starts_with("==> ") {
        write(dir, &[("app.osk", body)]);
        return "app.osk".to_string();
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        match line
            .strip_prefix("==> ")
            .and_then(|rest| rest.strip_suffix(" <=="))
        {
            Some(path) => files.push((path.to_string(), String::new())),
            None => {
                let (_, text) = files.last_mut().expect("a file");
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    write(dir, &files);
    ".".to_string()
}

#[test]
fn explain_gives_the_applicability_of_a_codes_fix_and_null_without_one() {
    assert_eq!(
        explained("E0200").get("fix_applicability"),
        Some(&string("maybe"))
    );
    assert_eq!(
        explained("E0253").get("fix_applicability"),
        Some(&string("placeholders"))
    );
    assert_eq!(
        explained("E0210").get("fix_applicability"),
        Some(&string("machine"))
    );
    assert_eq!(
        explained("E0300").get("fix_applicability"),
        Some(&Json::Null)
    );
}

/// Codes with a fix whose example cannot show it: `E0170` has no
/// example, since no program can reach it.
const FIX_NOT_SHOWN: [&str; 1] = ["E0170"];

/// Programs that get the fix of a code whose catalog example has none,
/// because the fix needs more than the example shows, such as a name
/// close enough to another.
const FIX_EXAMPLES: [(&str, &str); 4] = [
    (
        "E0227",
        "struct Point { x: Int, y: Int }\n\nfn main() {\n    let p = Point(x: 1, y: 2)\n    let z = p.xx\n}\n",
    ),
    (
        "E0265",
        "fn same<T>(a: T, b: T) -> Bool {\n    a == b\n}\n\nfn main() {}\n",
    ),
    (
        "E0400",
        "==> oska.toml <==\n\u{feff}[package]\nname = \"demo\"\nversion = \"0.1.0\"\n==> src/main.osk <==\nfn main() {}\n",
    ),
    (
        "E0402",
        "==> oska.toml <==\n[package]\nname = \"demo\"\nversio = \"0.1.0\"\n==> src/main.osk <==\nfn main() {}\n",
    ),
];

#[test]
fn each_codes_fix_has_the_applicability_its_catalog_entry_gives() {
    let mut problems = Vec::new();
    for code in checker_codes() {
        let entry = explained(&code);
        let catalogued = entry.get("fix_applicability").expect("a fix_applicability");
        let has_fix = entry.get("machine_fix") == Some(&Json::Bool(true));
        match (has_fix, catalogued) {
            (true, Json::Str(name))
                if ["machine", "maybe", "placeholders"].contains(&name.as_str()) => {}
            (false, Json::Null) => continue,
            _ => {
                problems.push(format!(
                    "{code}: fix {has_fix} with applicability {catalogued:?}"
                ));
                continue;
            }
        }
        let example = match (
            FIX_EXAMPLES.iter().find(|(c, _)| *c == code),
            entry.get("example"),
        ) {
            (Some((_, example)), _) => example.to_string(),
            (None, Some(Json::Str(example))) => example.clone(),
            _ => {
                if !FIX_NOT_SHOWN.contains(&code.as_str()) {
                    problems.push(format!("{code}: no example shows its fix"));
                }
                continue;
            }
        };
        let dir = scratch(&format!("catalog_{code}"));
        let path = write_example(&dir, &example);
        let diagnostics = checked(&dir, &path, &["--release"]);
        let emitted: Vec<&Json> = diagnostics
            .iter()
            .filter(|d| d.get("code") == Some(&string(&code)))
            .filter_map(|d| d.get("fix")?.get("applicability"))
            .collect();
        if emitted.is_empty() && !FIX_NOT_SHOWN.contains(&code.as_str()) {
            problems.push(format!("{code}: its example has no fix to compare"));
        }
        for found in emitted {
            if found != catalogued {
                problems.push(format!(
                    "{code}: the catalog says {catalogued:?}, the fix says {found:?}"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
