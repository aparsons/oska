// SPDX-License-Identifier: Apache-2.0
//! Native confinement on Windows only (#178), run on both backends
//! through the differential harness: what `native_files.rs` cannot plant
//! on Linux and macOS, and the paths only Windows writes. The
//! interpreter's own tests of the same cases are
//! `oska-run/tests/race_free_walks.rs` and `windows_names.rs`.
//!
//! - **Junctions** at each position of a path, refused as a symlink is.
//! - **Device names** (`CON`, `nul.txt`) and **alternate data streams**
//!   (`a.txt:hidden`), which are no plain file beneath the root.
//! - **A name too long** for `NtCreateFile`'s 16-bit length.
//! - **A root named by a drive path** (`C:\...`), with `\` or `/`.
//! - **A path beneath a root** read as Rust reads it on Windows: `\`
//!   separates, and a drive or a leading `\` makes it absolute.
//!
//! Every case starts from the tree `native_files.rs` uses:
//!
//! ```text
//! root/inside.txt         "inside text"
//! root/sub/nested.txt     "nested text"
//! outside/secret.txt      "outside text"
//! ```

#![cfg(all(windows, target_arch = "x86_64"))]

mod differential;

use differential::{assert_same, Case, Outcome};
use std::path::Path;
use std::process::Command;

/// The helpers every program has: what an operation gave, as one line.
const SHOWN: &str = r#"
fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// A program whose `main` has `io`, `fs`, `read` (`fs.read_only()`),
/// `write` (`fs.write_only()`) and `args`, and runs `body`.
fn program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, fs: Fs, args: Args) -> Result<(), IoError> {{\n\
         \x20   let read = fs.read_only()\n\
         \x20   let write = fs.write_only()\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n{SHOWN}"
    )
}

/// The standard tree with `body` as the program.
fn case(name: &str, body: &str) -> Case {
    Case::file(&format!("winfiles_{name}"), &program(body))
        .with_file("root/inside.txt", "inside text")
        .with_file("root/sub/nested.txt", "nested text")
        .with_file("outside/secret.txt", "outside text")
}

/// Run `case` on both backends, require the same outcome and a clean
/// exit, and return it.
fn same(case: &Case) -> Outcome {
    let outcome = assert_same(case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stderr, "", "{outcome:#?}");
    outcome
}

/// A junction at `dir/at` to the directory `dir/target`.
fn junction(dir: &Path, at: &str, target: &str) {
    let plain = |path: std::path::PathBuf| path.display().to_string().replace('/', "\\");
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(plain(dir.join(at)))
        .arg(plain(dir.join(target)))
        .output()
        .expect("cmd should run");
    assert!(made.status.success(), "mklink /J failed: {made:?}");
}

/// The words a walk refuses a reparse point at `path` with.
fn symlink(path: &str) -> String {
    format!("REFUSED: `{path}` is a symlink, and confined access never follows one")
}

fn device(name: &str) -> String {
    format!("REFUSED: `{name}` is a Windows device name, and confined access never opens one")
}

fn stream(name: &str) -> String {
    format!(
        "REFUSED: `{name}` holds a `:`, which names an alternate data stream on Windows, \
         and confined access never opens one"
    )
}

#[test]
fn a_junction_at_any_component_is_refused_and_nothing_escapes() {
    let case = case(
        "junctions",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("out/secret.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("sub/out/secret.txt"))))?
    io.println(listed(root.read_only().list_dir(Path.new("out"))))?
    io.println(done(root.write_only().write(Path.new("out/planted.txt"), "pwned")))?
    io.println(done(root.write_only().create_dir(Path.new("sub/out/planted"))))?
    io.println(done(root.write_only().create_dir(Path.new("out"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("alias/nested.txt"))))?
    io.println(shown(root.read_only().restrict_to(Path.new("out")).read_to_string(Path.new("secret.txt"))))?
    io.println(shown(read.restrict_to_file("root/out").read_to_string()))?
    io.println(shown(read.restrict_to("root-link").read_to_string(Path.new("inside.txt"))))?"#,
    )
    .with_setup(|dir| {
        junction(dir, "root/out", "outside");
        junction(dir, "root/sub/out", "outside");
        junction(dir, "root/alias", "root/sub");
        junction(dir, "root-link", "root");
    });
    // A junction's target is absolute, so the two runs' trees differ in
    // it: what the programs printed is compared, and each tree checked.
    let (interpreted, outcome) = differential::run(&case);
    assert_eq!(
        interpreted.stdout, outcome.stdout,
        "{interpreted:#?}\n{outcome:#?}"
    );
    assert_eq!(interpreted.exit_code, Some(0), "{interpreted:#?}");
    assert!(!interpreted.tree.contains_key("outside/planted.txt"));
    assert!(!interpreted.tree.contains_key("outside/planted/"));
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[..7],
        [
            symlink("out"),
            symlink("sub/out"),
            symlink("out"),
            symlink("out"),
            symlink("sub/out"),
            symlink("out"),
            symlink("alias"),
        ],
        "{outcome:#?}"
    );
    assert_eq!(lines.len(), 10, "{outcome:#?}");
    for line in &lines[7..] {
        assert!(
            line.starts_with("REFUSED: ") && line.contains("is a symlink"),
            "{outcome:#?}"
        );
    }
    assert!(!outcome.tree.contains_key("outside/planted.txt"));
    assert!(!outcome.tree.contains_key("outside/planted/"));
}

#[test]
fn a_device_name_or_a_stream_is_refused_in_every_operation() {
    let case = case(
        "names",
        r#"    let root = fs.restrict_to("root")
    for name in ["CON", "nul", "Aux", "PRN", "COM1", "lpt9", "nul.txt", "CON.log", "com3.tar.gz", "LPT1 .txt", "con/inside.txt", "inside.txt:hidden", "inside.txt::$DATA", "sub/a:b"] {
        io.println(shown(root.read_only().read_to_string(Path.new(name))))?
        io.println(done(root.write_only().write(Path.new(name), "text")))?
    }
    io.println(done(root.write_only().create_dir(Path.new("aux"))))?
    io.println(listed(root.read_only().list_dir(Path.new("nul"))))?
    io.println(done(root.write_only().restrict_to_file(Path.new("CON")).write("text")))?
    io.println(listed(root.read_only().restrict_to(Path.new("prn")).list_dir(Path.new("."))))?
    for name in ["console.txt", "com10", "nul_file"] {
        io.println(done(root.write_only().write(Path.new(name), name)))?
    }
    io.println(listed(root.read_only().list_dir(Path.new("."))))?"#,
    );
    let outcome = same(&case);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    let devices = [
        "CON",
        "nul",
        "Aux",
        "PRN",
        "COM1",
        "lpt9",
        "nul.txt",
        "CON.log",
        "com3.tar.gz",
        "LPT1 .txt",
        "con",
    ];
    for (i, name) in devices.iter().enumerate() {
        assert_eq!(lines[2 * i], device(name), "{outcome:#?}");
        assert_eq!(lines[2 * i + 1], device(name), "{outcome:#?}");
    }
    for (i, name) in ["inside.txt:hidden", "inside.txt::$DATA", "a:b"]
        .iter()
        .enumerate()
    {
        let at = 2 * (devices.len() + i);
        assert_eq!(lines[at], stream(name), "{outcome:#?}");
        // `a:b` is a drive to Rust's `Path`, so a write's last name is no
        // file name at all.
        let written = match *name {
            "a:b" => "REFUSED: `sub/a:b` does not end in a file name; \
                      a write, a new directory or a file needs one"
                .to_string(),
            _ => stream(name),
        };
        assert_eq!(lines[at + 1], written, "{outcome:#?}");
    }
    assert_eq!(lines[28], device("aux"));
    assert_eq!(lines[29], device("nul"));
    assert_eq!(
        lines[30],
        format!("REFUSED: cannot restrict to `CON`: {}", &device("CON")[9..])
    );
    assert!(lines[31].contains(&device("prn")[9..]), "{outcome:#?}");
    assert_eq!(lines[32..35], ["done", "done", "done"]);
    assert_eq!(lines[35], "com10,console.txt,inside.txt,nul_file,sub");
    assert_eq!(
        outcome.tree["root/inside.txt"].as_deref(),
        Some("inside text")
    );
}

#[test]
fn a_name_too_long_for_windows_is_refused_and_never_cut_short() {
    // 32769 UTF-16 units do not fit a 16-bit length in bytes: cut to its
    // low 16 bits, the name would be its first unit, `a`.
    let long = "a".repeat(32769);
    let src = program(&format!(
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("{long}"))))?
    io.println(done(root.write_only().write(Path.new("{long}"), "pwned")))?
    io.println(done(root.write_only().create_dir(Path.new("{long}"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("{long}/x"))))?"#
    ));
    let case = Case::file("winfiles_long", &src).with_file("root/a", "a text");
    let outcome = same(&case);
    let too_long = format!("REFUSED: `{long}`: a file name is too long");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines, [too_long.as_str(); 4], "{outcome:#?}");
    assert_eq!(outcome.tree["root/a"].as_deref(), Some("a text"));
}

#[test]
fn a_root_named_by_a_drive_path_is_opened_with_either_separator() {
    // The path differs between the two runs' directories, so both run in
    // the native one with it as an argument, and print no path.
    let case = case(
        "drive_root",
        r#"    let at = args.get(1).unwrap_or("").assume_trusted()
    let root = read.restrict_to("{at}\\root")
    io.println(shown(root.read_to_string(Path.new("sub/nested.txt"))))?
    io.println(listed(root.list_dir(Path.new("."))))?
    let slashed = read.restrict_to("{at}/root/sub")
    io.println(shown(slashed.read_to_string(Path.new("nested.txt"))))?
    io.println(shown(read.restrict_to_file("{at}\\root\\inside.txt").read_to_string()))?
    let link = read.restrict_to("{at}\\root-link")
    io.println(shown(link.read_to_string(Path.new("inside.txt"))))?"#,
    )
    .with_setup(|dir| junction(dir, "root-link", "root"));
    let (exe, dir) = differential::build_native(&case);
    let at = dir.display().to_string();
    assert!(at.as_bytes().get(1) == Some(&b':'), "a drive path: {at}");
    let native = Command::new(exe)
        .arg(&at)
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    let interpreted = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["run", "--interpret", "main.osk", "--"])
        .arg(&at)
        .current_dir(&dir)
        .output()
        .expect("the interpreter should run");
    assert_eq!(
        interpreted.stdout, native.stdout,
        "{interpreted:?}\n{native:?}"
    );
    let seen = String::from_utf8_lossy(&native.stdout);
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(
        lines[..4],
        [
            "nested text",
            "inside.txt,sub",
            "nested text",
            "inside text"
        ],
        "{native:?}"
    );
    assert!(lines[4].contains("`root-link` is a symlink"), "{native:?}");
}

#[test]
fn a_path_beneath_a_root_is_read_as_windows_reads_it() {
    let case = case(
        "separators",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("sub\\nested.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("sub\\..\\inside.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("\\inside.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("C:inside.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("..\\outside\\secret.txt"))))?
    io.println(done(root.write_only().write(Path.new("sub\\"), "x")))?
    io.println(done(root.write_only().write(Path.new("sub/a:b"), "x")))?"#,
    );
    let outcome = same(&case);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines[..2], ["nested text", "inside text"], "{outcome:#?}");
    for line in &lines[2..4] {
        assert!(line.contains("is absolute"), "{outcome:#?}");
    }
    assert!(lines[4].contains("leads outside"), "{outcome:#?}");
    for line in &lines[5..] {
        assert!(line.contains("does not end in a file name"), "{outcome:#?}");
    }
}
