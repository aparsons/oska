//! Native confined file capabilities (ticket #137), run on both backends
//! through the differential harness (seam 1): `Fs`, `FsRead` and
//! `FsWrite` attenuations, `Dir`, `DirRead`, `DirWrite`, `FileRead` and
//! `FileWrite`, with every refusal of the interpreter's real-OS
//! confinement suites (`oska-run/tests/confined_reads.rs`,
//! `confined_writes.rs`, `directory_capabilities.rs`,
//! `file_capabilities.rs`, `hard_links.rs`, `whole_filesystem.rs`).
//!
//! Every case starts from the same tree, relative to the working
//! directory so both backends see the same paths:
//!
//! ```text
//! root/inside.txt         "inside text"
//! root/sub/nested.txt     "nested text"
//! outside/secret.txt      "outside text"
//! ```
//!
//! Symlinks, hard links and FIFOs are planted by a setup function; the
//! harness records a symlink by its target and never follows one, so a
//! write through one that escaped would show in the tree.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case, Outcome};
use std::path::Path;

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
    tree(Case::file(&format!("files_{name}"), &program(body)))
}

/// `case` with the standard tree's files.
fn tree(case: Case) -> Case {
    case.with_file("root/inside.txt", "inside text")
        .with_file("root/sub/nested.txt", "nested text")
        .with_file("outside/secret.txt", "outside text")
}

/// Run `case` on both backends, require the same outcome and a clean
/// exit, and return what it printed.
fn printed(case: &Case) -> String {
    let outcome: Outcome = assert_same(case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stderr, "", "{outcome:#?}");
    outcome.stdout
}

/// A symlink at `dir/at` to `target`.
fn link(dir: &Path, at: &str, target: &str) {
    std::os::unix::fs::symlink(target, dir.join(at)).expect("create a symlink");
}

// ---- Reading ------------------------------------------------------------------

#[test]
fn a_directory_capability_reads_files_beneath_its_root() {
    let case = case(
        "read",
        r#"    let root = read.restrict_to("root")
    io.println(shown(root.read_to_string(Path.new("inside.txt"))))?
    io.println(shown(root.read_to_string(Path.new("sub/nested.txt"))))?
    io.println(shown(root.restrict_to(Path.new("sub")).read_to_string(Path.new("nested.txt"))))?
    io.println(shown(root.read_to_string(Path.new("missing.txt"))))?"#,
    );
    assert_eq!(
        printed(&case),
        "inside text\nnested text\nnested text\n\
         REFUSED: `missing.txt`: No such file or directory (os error 2)\n"
    );
}

// ---- Symlinks, at every component (`confined_reads.rs`) ------------------------

/// What reading each of `paths` beneath `root` printed, one line each.
fn reads(name: &str, root: &str, paths: &[&str], setup: fn(&Path)) -> String {
    let body: String = paths
        .iter()
        .map(|path| {
            format!(
                "    io.println(shown(read.restrict_to(\"{root}\").read_to_string(Path.new(\"{path}\"))))?\n"
            )
        })
        .collect();
    printed(&case(name, &body).with_setup(setup))
}

#[test]
fn a_symlink_at_any_component_is_refused_dangling_or_pointing_inside() {
    let seen = reads(
        "read_links",
        "root",
        &[
            "out/secret.txt",
            "sub/out/secret.txt",
            "sub/escape.txt",
            "gone.txt",
            "gone/secret.txt",
            "alias.txt",
            "alias/nested.txt",
            "relative.txt",
        ],
        |dir| {
            link(dir, "root/out", "../outside");
            link(dir, "root/sub/out", "../../outside");
            link(dir, "root/sub/escape.txt", "../../outside/secret.txt");
            link(dir, "root/gone.txt", "../nothing-here");
            link(dir, "root/gone", "../nothing-here");
            link(dir, "root/alias.txt", "inside.txt");
            link(dir, "root/alias", "sub");
            link(dir, "root/relative.txt", "sub/nested.txt");
        },
    );
    assert_eq!(
        seen,
        "REFUSED: `out` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `sub/out` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `sub/escape.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `gone.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `gone` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `alias.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `alias` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `relative.txt` is a symlink, and confined access never follows one\n"
    );
}

#[test]
fn dot_dot_resolves_as_written_and_never_climbs_above_the_root() {
    let seen = reads(
        "read_dots",
        "root",
        &[
            "sub/../inside.txt",
            "./sub/./../sub/nested.txt",
            "out/../inside.txt",
            "../outside/secret.txt",
            "sub/../../outside/secret.txt",
            "..",
            ".",
            "sub/..",
            "/etc/passwd",
            "",
            "sub//nested.txt",
            "sub/nested.txt/",
        ],
        |dir| link(dir, "root/out", "../outside"),
    );
    assert_eq!(
        seen,
        "inside text\nnested text\ninside text\n\
         REFUSED: `../outside/secret.txt` leads outside the capability's root\n\
         REFUSED: `sub/../../outside/secret.txt` leads outside the capability's root\n\
         REFUSED: `..` leads outside the capability's root\n\
         REFUSED: `.` names the directory capability's root itself, not a file\n\
         REFUSED: `sub/..` names the directory capability's root itself, not a file\n\
         REFUSED: `/etc/passwd` is absolute; a directory capability takes paths relative to its root\n\
         REFUSED: an empty path names no file\n\
         nested text\nnested text\n"
    );
}

#[test]
fn a_root_is_trusted_text_but_its_last_component_is_never_a_symlink() {
    let case = case(
        "roots",
        r#"    for root in ["root-link", "root-link/", "root-link/.", "via/root", "./root/", "root/sub/..", ".", "missing", "root/inside.txt", "root/inside.txt/x", "", "/"] {
        let dir = read.restrict_to(root)
        io.println(shown(dir.read_to_string(Path.new("inside.txt"))))?
        io.println(listed(dir.list_dir(Path.new("sub"))))?
    }
    let narrowed = read.restrict_to("root")
    for to in ["out", "alias", "missing", "inside.txt", "./sub/", "sub/../..", "/root"] {
        io.println(shown(narrowed.restrict_to(Path.new(to)).read_to_string(Path.new("nested.txt"))))?
    }"#,
    )
    .with_setup(|dir| {
        link(dir, "root-link", "root");
        link(dir, "via", ".");
        link(dir, "root/out", "../outside");
        link(dir, "root/alias", "sub");
    });
    let seen = printed(&case);
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 31, "{seen}");
    for line in &lines[0..6] {
        assert!(line.contains("`root-link` is a symlink"), "{seen}");
    }
    assert_eq!(
        lines[6..12],
        [
            "inside text",
            "nested.txt",
            "inside text",
            "nested.txt",
            "inside text",
            "nested.txt"
        ],
        "{seen}"
    );
    assert_eq!(
        lines[14],
        "REFUSED: cannot restrict to `missing`: `missing`: No such file or directory (os error 2)"
    );
    assert_eq!(
        lines[16],
        "REFUSED: cannot restrict to `root/inside.txt`: `inside.txt` is a symlink or not a directory, \
         and confined access never follows a symlink"
    );
    assert_eq!(
        lines[18],
        "REFUSED: cannot restrict to `root/inside.txt/x`: `root/inside.txt`: Not a directory (os error 20)"
    );
    assert_eq!(
        lines[20],
        "REFUSED: cannot restrict to ``: an empty path names no directory"
    );
    assert!(lines[26].contains("No such file or directory"), "{seen}");
    assert_eq!(lines[28], "nested text", "{seen}");
}

#[test]
fn a_fifo_is_refused_rather_than_waited_on_and_a_directory_is_no_file() {
    let case = case(
        "fifo",
        r#"    let root = read.restrict_to("root")
    io.println(shown(root.read_to_string(Path.new("pipe"))))?
    io.println(shown(root.read_to_string(Path.new("sub"))))?
    io.println(shown(read.restrict_to_file("root/pipe").read_to_string()))?
    let out = write.restrict_to("root")
    io.println(done(out.write(Path.new("pipe"), "x")))?
    io.println(done(out.write(Path.new("sub"), "x")))?
    io.println(done(out.create_dir(Path.new("pipe"))))?
    io.println(done(out.create_dir(Path.new("inside.txt"))))?
    io.println(listed(root.list_dir(Path.new("pipe"))))?"#,
    )
    .with_setup(|dir| {
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join("root/pipe"))
            .status()
            .expect("mkfifo should run");
        assert!(made.success());
    });
    assert_eq!(
        printed(&case),
        "REFUSED: `pipe` is not a regular file\n\
         REFUSED: `sub` is not a regular file\n\
         REFUSED: `pipe` is not a regular file\n\
         REFUSED: `pipe`: No such device or address (os error 6)\n\
         REFUSED: `sub`: Is a directory (os error 21)\n\
         REFUSED: `pipe` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `inside.txt` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `pipe` is a symlink or not a directory, and confined access never follows a symlink\n"
    );
}

// ---- Writes, `create_dir` and `list_dir` (`confined_writes.rs`) ----------------

/// The symlinks `confined_writes.rs` plants beneath the root.
fn plant_links(dir: &Path) {
    link(dir, "root/out", "../outside");
    link(dir, "root/sub/out", "../../outside");
    link(dir, "root/alias", "sub");
    link(dir, "root/escape.txt", "../outside/secret.txt");
    link(dir, "root/plant.txt", "../outside/planted.txt");
    link(dir, "root/sub/plant.txt", "../../outside/planted.txt");
    link(dir, "root/alias.txt", "inside.txt");
    link(dir, "root/dangling", "../outside/planted");
}

/// Every line of `seen` is refused, saying `why`, and there are `lines`.
#[track_caller]
fn assert_all_refused(seen: &str, lines: usize, why: &str) {
    assert_eq!(seen.lines().count(), lines, "{seen}");
    for line in seen.lines() {
        assert!(
            line.starts_with("REFUSED: ") && line.contains(why),
            "should be refused for `{why}`: {seen}"
        );
    }
}

/// A body that writes `pwned` beneath `root` at each of `paths`.
fn writes(paths: &[&str]) -> String {
    paths
        .iter()
        .map(|path| {
            format!(
                "    io.println(done(write.restrict_to(\"root\").write(Path.new(\"{path}\"), \"pwned\")))?\n"
            )
        })
        .collect()
}

#[test]
fn a_write_creates_or_replaces_a_file_beneath_the_root() {
    let case = case(
        "write",
        r#"    let out = write.restrict_to("root")
    io.println(done(out.write(Path.new("sub/new.txt"), "fresh")))?
    io.println(done(out.write(Path.new("inside.txt"), "again")))?
    io.println(done(out.write(Path.new("sub/../top.txt"), "top")))?
    io.println(done(out.write(Path.new("empty.txt"), "")))?
    io.println(done(out.write(Path.new("missing/new.txt"), "x")))?
    io.println(shown(read.restrict_to("root").read_to_string(Path.new("inside.txt"))))?"#,
    );
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        "done\ndone\ndone\ndone\n\
         REFUSED: `missing`: No such file or directory (os error 2)\n\
         again\n"
    );
    assert_eq!(outcome.tree["root/inside.txt"].as_deref(), Some("again"));
    assert_eq!(outcome.tree["root/sub/new.txt"].as_deref(), Some("fresh"));
    assert_eq!(outcome.tree["root/top.txt"].as_deref(), Some("top"));
    assert_eq!(outcome.tree["root/empty.txt"].as_deref(), Some(""));
}

#[test]
fn a_write_through_a_symlink_anywhere_creates_nothing() {
    let case = case(
        "write_links",
        &writes(&[
            "plant.txt",
            "sub/plant.txt",
            "escape.txt",
            "alias.txt",
            "out/new.txt",
            "sub/out/new.txt",
            "alias/new.txt",
            "out/x/../new.txt",
        ]),
    )
    .with_setup(plant_links);
    let outcome = assert_same(&case);
    assert_all_refused(&outcome.stdout, 8, "symlink");
    assert_eq!(
        outcome.tree["outside/secret.txt"].as_deref(),
        Some("outside text")
    );
    assert!(!outcome.tree.contains_key("outside/planted.txt"));
    assert!(!outcome.tree.contains_key("root/sub/new.txt"));
}

#[test]
fn a_write_above_the_root_or_not_ending_in_a_name_is_refused() {
    let case = case(
        "write_paths",
        &writes(&[
            "../outside/new.txt",
            "sub/../../outside/new.txt",
            "/tmp/new.txt",
            "",
            "new/",
            "sub/.",
            ".",
            "sub/..",
            "dangling/",
            "dangling/.",
            "..",
        ]),
    )
    .with_setup(plant_links);
    assert_eq!(
        printed(&case),
        "REFUSED: `../outside/new.txt` leads outside the capability's root\n\
         REFUSED: `sub/../../outside/new.txt` leads outside the capability's root\n\
         REFUSED: `/tmp/new.txt` is absolute; a directory capability takes paths relative to its root\n\
         REFUSED: an empty path names no file\n\
         REFUSED: `new/` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `sub/.` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `.` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `sub/..` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `dangling/` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `dangling/.` does not end in a file name; a write, a new directory or a file needs one\n\
         REFUSED: `..` leads outside the capability's root\n"
    );
}

#[test]
fn create_dir_makes_a_directory_keeps_one_and_refuses_symlinks() {
    let case = case(
        "create_dir",
        r#"    let out = write.restrict_to("root")
    for path in ["new", "sub/deeper", "sub/../again", "sub", "out/new", "sub/out/new", "alias/new", "dangling", "alias", "out", "..", "../outside/new", "/tmp/new", "", "missing/child", "inside.txt", "new/"] {
        io.println(done(out.create_dir(Path.new(path))))?
    }"#,
    )
    .with_setup(plant_links);
    let outcome = assert_same(&case);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines[..4], ["done", "done", "done", "done"], "{outcome:#?}");
    for line in &lines[4..10] {
        assert!(line.contains("symlink"), "{outcome:#?}");
    }
    assert_eq!(
        lines[14],
        "REFUSED: `missing`: No such file or directory (os error 2)"
    );
    for made in ["root/new/", "root/sub/deeper/", "root/again/"] {
        assert!(outcome.tree.contains_key(made), "{outcome:#?}");
    }
    assert!(!outcome.tree.contains_key("outside/planted/"));
}

#[test]
fn list_dir_lists_sorted_by_bytes_and_never_follows() {
    let case = case(
        "list_dir",
        r#"    let out = write.restrict_to("root")
    for name in ["Zebra.txt", "zeta", "Ärger", "a b", "_x", "10", "9", "émile"] {
        io.println(done(out.write(Path.new("sub/{name}"), name)))?
    }
    let root = read.restrict_to("root")
    for path in [".", "sub", "sub/.", "sub/../sub", "out", "sub/out", "alias", "alias/.", "out/x/..", "dangling", "..", "../outside", "/tmp", "", "inside.txt", "missing"] {
        io.println(listed(root.list_dir(Path.new(path))))?
    }
    let twice = read.restrict_to("root/sub")
    let first = twice.list_dir(Path.new(".")).unwrap_or([]).len()
    let second = twice.list_dir(Path.new(".")).unwrap_or([]).len()
    io.println("{first} {second}")?"#,
    )
    .with_setup(plant_links);
    let outcome = assert_same(&case);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[8],
        "alias,alias.txt,dangling,escape.txt,inside.txt,out,plant.txt,sub"
    );
    assert_eq!(
        lines[9],
        "10,9,Zebra.txt,_x,a b,nested.txt,out,plant.txt,zeta,Ärger,émile"
    );
    assert_eq!(lines[10], lines[9]);
    assert_eq!(lines[11], lines[9]);
    for line in &lines[12..18] {
        assert!(line.contains("symlink"), "{outcome:#?}");
    }
    assert_eq!(lines[24], "11 11");
}

#[test]
fn a_symlinked_root_writes_creates_and_lists_nothing() {
    let case = case(
        "symlinked_root",
        r#"    for root in ["root-link", "outside-link"] {
        let dir = fs.restrict_to(root)
        io.println(done(dir.write_only().write(Path.new("new.txt"), "x")))?
        io.println(done(dir.write_only().create_dir(Path.new("new"))))?
        io.println(listed(dir.read_only().list_dir(Path.new("."))))?
        io.println(shown(dir.read_only().read_to_string(Path.new("inside.txt"))))?
        io.println(shown(dir.read_only().restrict_to(Path.new("sub")).read_to_string(Path.new("nested.txt"))))?
        io.println(done(dir.write_only().restrict_to_file(Path.new("f.txt")).write("x")))?
    }"#,
    )
    .with_setup(|dir| {
        link(dir, "root-link", "root");
        link(dir, "outside-link", "outside");
    });
    let outcome = assert_same(&case);
    assert_all_refused(&outcome.stdout, 12, "is a symlink");
    assert!(!outcome.tree.contains_key("root/new.txt"));
    assert!(!outcome.tree.contains_key("outside/new.txt"));
}

// ---- Narrowing from untrusted text (`directory_capabilities.rs`) ----------------

#[test]
fn a_narrowing_from_untrusted_text_stays_beneath_the_root() {
    let case = case(
        "narrowing",
        r#"    let root = fs.restrict_to("root")
    var i = 1
    while i < 9 {
        let user = args.get(i).unwrap_or("")
        let narrowed = root.restrict_to(Path.from(user))
        io.println(shown(narrowed.read_only().read_to_string(Path.new("nested.txt"))))?
        io.println(done(narrowed.write_only().write(Path.new("planted.txt"), "pwned")))?
        io.println(done(narrowed.write_only().create_dir(Path.new("planted"))))?
        i = i + 1
    }"#,
    )
    .with_args(&[
        "sub",
        "./sub/",
        "out",
        "sub/out",
        "alias",
        "gone",
        "../outside",
        "sub/../..",
    ])
    .with_setup(|dir| {
        link(dir, "root/out", "../outside");
        link(dir, "root/sub/out", "../../outside");
        link(dir, "root/alias", "sub");
        link(dir, "root/gone", "../nothing-here");
    });
    let outcome = assert_same(&case);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[..6],
        ["nested text", "done", "done", "nested text", "done", "done"]
    );
    for line in &lines[6..18] {
        assert!(
            line.starts_with("REFUSED: cannot restrict to `") && line.contains("symlink"),
            "{outcome:#?}"
        );
    }
    for line in &lines[18..] {
        assert!(
            line.contains("leads outside the capability's root"),
            "{outcome:#?}"
        );
    }
    assert!(!outcome.tree.contains_key("outside/planted.txt"));
    assert!(!outcome.tree.contains_key("outside/planted/"));
}

// ---- File capabilities (`file_capabilities.rs`) ----------------------------------

#[test]
fn file_capabilities_read_and_write_their_one_file() {
    let case = case(
        "file_round_trip",
        r#"    let inside = read.restrict_to_file("root/inside.txt")
    io.println(shown(inside.read_to_string()))?
    let sub = read.restrict_to("root")
    io.println(shown(sub.restrict_to_file(Path.new("sub/nested.txt")).read_to_string()))?
    io.println(shown(sub.restrict_to_file(Path.new("inside.txt")).read_to_string()))?
    let out = write.restrict_to_file("root/new.txt")
    io.println(done(out.write("a long first draft")))?
    io.println(done(out.write("short")))?
    io.println(shown(read.restrict_to_file("./root/./new.txt").read_to_string()))?
    io.println(shown(read.restrict_to_file("root/missing.txt").read_to_string()))?"#,
    );
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        "inside text\nnested text\ninside text\ndone\ndone\nshort\n\
         REFUSED: `missing.txt`: No such file or directory (os error 2)\n"
    );
    assert_eq!(outcome.tree["root/new.txt"].as_deref(), Some("short"));
}

#[test]
fn a_file_capability_never_follows_a_symlinked_name_or_parent() {
    let case = case(
        "file_links",
        r#"    for name in ["root/link.txt", "root/alias.txt", "root/dangling.txt"] {
        io.println(shown(read.restrict_to_file(name).read_to_string()))?
        io.println(done(write.restrict_to_file(name).write("pwned")))?
    }
    let readable = read.restrict_to("root")
    let writable = write.restrict_to("root")
    var i = 1
    while i < 8 {
        let name = args.get(i).unwrap_or("")
        io.println(shown(readable.restrict_to_file(Path.from(name)).read_to_string()))?
        io.println(done(writable.restrict_to_file(Path.from(name)).write("pwned")))?
        i = i + 1
    }"#,
    )
    .with_args(&[
        "link.txt",
        "sub/link.txt",
        "alias.txt",
        "dangling.txt",
        "out/secret.txt",
        "sub/out/secret.txt",
        "alias/nested.txt",
    ])
    .with_setup(|dir| {
        link(dir, "root/link.txt", "../outside/secret.txt");
        link(dir, "root/sub/link.txt", "../../outside/secret.txt");
        link(dir, "root/alias.txt", "inside.txt");
        link(dir, "root/dangling.txt", "../outside/planted.txt");
        link(dir, "root/out", "../outside");
        link(dir, "root/sub/out", "../../outside");
        link(dir, "root/alias", "sub");
    });
    let outcome = assert_same(&case);
    assert_all_refused(&outcome.stdout, 20, "symlink");
    assert!(!outcome.tree.contains_key("outside/planted.txt"));
    assert_eq!(
        outcome.tree["outside/secret.txt"].as_deref(),
        Some("outside text")
    );
}

#[test]
fn a_broken_file_or_directory_root_fails_every_operation() {
    let case = case(
        "broken_roots",
        r#"    io.println(shown(read.restrict_to_file("root/nodir/x.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("root/nodir/x.txt").write("x")))?
    io.println(shown(read.restrict_to_file("root/sub").read_to_string()))?
    io.println(done(write.restrict_to_file("root/sub").write("x")))?
    io.println(shown(read.restrict_to_file("root/inside.txt/x").read_to_string()))?
    io.println(shown(read.restrict_to_file("root/..").read_to_string()))?
    io.println(shown(read.restrict_to_file("/").read_to_string()))?
    io.println(shown(read.restrict_to_file("").read_to_string()))?
    let docs = read.restrict_to("root/inside.txt")
    io.println(shown(docs.read_to_string(Path.new("."))))?
    io.println(shown(docs.read_to_string(Path.new("inside.txt"))))?
    io.println(shown(docs.restrict_to_file(Path.new("x")).read_to_string()))?
    io.println(shown(docs.restrict_to(Path.new("x")).read_to_string(Path.new("y"))))?
    let narrowed = read.restrict_to("root").restrict_to(Path.new("inside.txt"))
    io.println(shown(narrowed.read_to_string(Path.new("."))))?
    let site = write.restrict_to("root/inside.txt")
    io.println(done(site.write(Path.new("x.txt"), "x")))?
    let beneath = read.restrict_to("root")
    for name in ["nodir/x.txt", "", "/x", "../x", "sub/", "x/.."] {
        io.println(shown(beneath.restrict_to_file(Path.new(name)).read_to_string()))?
    }"#,
    );
    let outcome = assert_same(&case);
    assert_all_refused(&outcome.stdout, 20, "");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[0],
        "REFUSED: cannot restrict to `root/nodir/x.txt`: `root/nodir`: No such file or directory (os error 2)"
    );
    assert_eq!(lines[2], "REFUSED: `sub` is not a regular file");
    assert_eq!(
        lines[5],
        "REFUSED: cannot restrict to `root/..`: `root/..` does not end in a file name"
    );
    assert!(!outcome.tree.contains_key("root/nodir/"));
    assert!(!outcome.tree.contains_key("root/x.txt"));
}

// ---- Hard links (`hard_links.rs`) --------------------------------------------------

#[test]
fn a_file_with_more_than_one_hard_link_is_never_read_or_emptied() {
    let case = case(
        "hard_links",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("leak.txt"))))?
    io.println(done(root.write_only().write(Path.new("leak.txt"), "overwritten")))?
    io.println(shown(read.restrict_to_file("root/leak.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("root/leak.txt").write("overwritten")))?
    io.println(shown(root.read_only().restrict_to_file(Path.new("leak.txt")).read_to_string()))?
    io.println(done(root.write_only().restrict_to_file(Path.new("leak.txt")).write("overwritten")))?
    io.println(shown(root.read_only().read_to_string(Path.new("inside.txt"))))?
    io.println(done(root.write_only().write(Path.new("inside.txt"), "rewritten")))?
    io.println(done(root.write_only().write(Path.new("new.txt"), "new text")))?
    io.println(shown(root.read_only().read_to_string(Path.new("new.txt"))))?"#,
    )
    .with_setup(|dir| {
        std::fs::hard_link(dir.join("outside/secret.txt"), dir.join("root/leak.txt"))
            .expect("create a hard link");
    });
    let outcome = assert_same(&case);
    let refusal =
        "REFUSED: `leak.txt` has more than one hard link, and confined access never opens one\n";
    assert_eq!(
        outcome.stdout,
        format!("{}inside text\ndone\ndone\nnew text\n", refusal.repeat(6))
    );
    assert_eq!(
        outcome.tree["outside/secret.txt"].as_deref(),
        Some("outside text")
    );
    assert_eq!(
        outcome.tree["root/inside.txt"].as_deref(),
        Some("rewritten")
    );
}

// ---- The whole filesystem (`whole_filesystem.rs`) ------------------------------

#[test]
fn the_whole_filesystem_is_still_confined_by_the_path_rules() {
    let case = case(
        "whole_paths",
        r#"    let everything = fs.restrict_to("/")
    io.println(shown(everything.read_only().read_to_string(Path.new("../up.txt"))))?
    io.println(shown(everything.read_only().read_to_string(Path.new("/etc/hostname"))))?
    io.println(shown(everything.read_only().read_to_string(Path.new("proc"))))?
    io.println(shown(fs.restrict_to("/proc/..").read_only().read_to_string(Path.new("."))))?"#,
    );
    assert_eq!(
        printed(&case),
        "REFUSED: `../up.txt` leads outside the capability's root\n\
         REFUSED: `/etc/hostname` is absolute; a directory capability takes paths relative to its root\n\
         REFUSED: `proc` is not a regular file\n\
         REFUSED: `.` names the directory capability's root itself, not a file\n"
    );
}

#[test]
fn restrict_to_slash_reads_and_writes_a_real_file() {
    // Natively only: the path from `/` names the case's own directory,
    // which differs between the two runs.
    let case = case(
        "whole_real",
        r#"    let at = args.get(1).unwrap_or("")
    let everything = fs.restrict_to("/")
    let text = everything.read_only().read_to_string(Path.from("{at}/root/inside.txt"))?
    everything.write_only().write(Path.from("{at}/root/out.txt"), text)?
    io.println(listed(everything.read_only().list_dir(Path.from("{at}/root"))))?"#,
    );
    let (exe, dir) = differential::build_native(&case);
    let relative = dir
        .strip_prefix("/")
        .expect("an absolute path")
        .display()
        .to_string();
    let output = std::process::Command::new(exe)
        .arg(&relative)
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "inside.txt,out.txt,sub\n",
        "{output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("root/out.txt")).expect("written"),
        "inside text"
    );
}

// ---- Swapped while the program runs (`file_capabilities.rs`) -------------------

/// Run `case`, whose program prints `ready` and then reads a line, on
/// both backends: once it is ready, `swap` changes its tree, and then it
/// goes on. Requires the same output from both, and returns it with the
/// native run's directory.
fn swapping(case: impl Fn(&str) -> Case, swap: fn(&Path)) -> (String, std::path::PathBuf) {
    let (exe, native_dir) = differential::build_native(&case("native"));
    let native = interact(std::process::Command::new(exe), &native_dir, swap);
    // A second tree for the interpreter, set up the same way.
    let (_, interpreted_dir) = differential::build_native(&case("interpreted"));
    let mut oskac = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"));
    oskac.arg("run").arg("main.osk");
    let interpreted = interact(oskac, &interpreted_dir, swap);
    assert_eq!(interpreted, native, "the backends differ");
    (native, native_dir)
}

/// Run `command` in `dir` until it prints `ready`, call `swap`, then let
/// it finish: everything it printed after `ready`.
fn interact(mut command: std::process::Command, dir: &Path, swap: fn(&Path)) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let mut child = command
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("a line");
    assert_eq!(line, "ready\n");
    swap(dir);
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"go\n").expect("write stdin");
    drop(stdin);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).expect("the rest");
    let status = child.wait().expect("the program should finish");
    assert!(status.success(), "{rest}");
    rest
}

/// A program that makes its capabilities with `before`, prints `ready`,
/// waits for a line on stdin, then runs `after`.
fn swap_case(name: &str, before: &str, after: &str) -> Case {
    let src = program(&format!(
        "{before}\n    io.println(\"ready\")?\n    let _ = stdin.read_line()\n{after}"
    ))
    .replace("args: Args)", "args: Args, stdin: Stdin)");
    tree(Case::file(&format!("files_{name}"), &src))
}

#[test]
fn a_file_swapped_for_a_symlink_after_the_capability_was_made_is_refused() {
    let case = |side: &str| {
        swap_case(
            &format!("swap_file_{side}"),
            r#"    let inside = read.restrict_to_file("root/inside.txt")
    let out = write.restrict_to_file("root/inside.txt")
    let dir = fs.restrict_to("root")"#,
            r#"    io.println(shown(inside.read_to_string()))?
    io.println(done(out.write("pwned")))?
    io.println(shown(dir.read_only().read_to_string(Path.new("inside.txt"))))?
    io.println(done(dir.write_only().write(Path.new("inside.txt"), "pwned")))?
    io.println(shown(dir.read_only().read_to_string(Path.new("sub/nested.txt"))))?
    io.println(listed(dir.read_only().list_dir(Path.new("sub"))))?
    io.println(done(dir.write_only().create_dir(Path.new("sub/new"))))?"#,
        )
    };
    let (seen, dir) = swapping(case, |dir| {
        std::fs::remove_file(dir.join("root/inside.txt")).expect("remove the file");
        link(dir, "root/inside.txt", "../outside/secret.txt");
        std::fs::rename(dir.join("root/sub"), dir.join("root/sub-moved")).expect("move sub");
        link(dir, "root/sub", "../outside");
    });
    assert_all_refused(&seen, 7, "symlink");
    assert_eq!(
        std::fs::read_to_string(dir.join("outside/secret.txt")).expect("read"),
        "outside text"
    );
    assert!(!dir.join("outside/new").exists());
}

#[test]
fn a_capability_whose_root_was_swapped_for_a_symlink_keeps_its_own_directory() {
    let case = |side: &str| {
        swap_case(
            &format!("swap_parent_{side}"),
            r#"    let inside = read.restrict_to_file("root/inside.txt")
    let out = write.restrict_to_file("root/inside.txt")
    let dir = fs.restrict_to("root")
    let sub = dir.restrict_to(Path.new("sub"))"#,
            r#"    io.println(shown(inside.read_to_string()))?
    io.println(done(out.write("rewritten")))?
    io.println(shown(inside.read_to_string()))?
    io.println(shown(dir.read_only().read_to_string(Path.new("sub/nested.txt"))))?
    io.println(listed(dir.read_only().list_dir(Path.new("."))))?
    io.println(done(sub.write_only().write(Path.new("new.txt"), "new")))?"#,
        )
        .with_file("outside/inside.txt", "decoy text")
    };
    let (seen, dir) = swapping(case, |dir| {
        std::fs::rename(dir.join("root"), dir.join("root-moved")).expect("move the root");
        link(dir, "root", "outside");
    });
    assert_eq!(
        seen,
        "inside text\ndone\nrewritten\nnested text\ninside.txt,sub\ndone\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("outside/inside.txt")).expect("read"),
        "decoy text"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("root-moved/sub/new.txt")).expect("read"),
        "new"
    );
}

// ---- Adversarial cases -----------------------------------------------------------

#[test]
fn a_nul_byte_anywhere_in_a_path_is_refused() {
    let case = case(
        "nul",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("in\0side.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("s\0ub/nested.txt"))))?
    io.println(done(root.write_only().write(Path.new("new\0.txt"), "x")))?
    io.println(done(root.write_only().create_dir(Path.new("new\0dir"))))?
    io.println(done(root.write_only().create_dir(Path.new("s\0ub/new"))))?
    io.println(listed(root.read_only().list_dir(Path.new("s\0ub"))))?
    io.println(shown(root.read_only().restrict_to(Path.new("s\0ub")).read_to_string(Path.new("nested.txt"))))?
    io.println(shown(read.restrict_to("ro\0ot").read_to_string(Path.new("inside.txt"))))?
    io.println(shown(read.restrict_to("root/s\0ub").read_to_string(Path.new("nested.txt"))))?
    io.println(shown(read.restrict_to_file("root/in\0side.txt").read_to_string()))?
    io.println(shown(read.restrict_to_file("ro\0ot/inside.txt").read_to_string()))?"#,
    );
    let outcome = assert_same(&case);
    assert_all_refused(
        &outcome.stdout,
        11,
        "a path holds a NUL byte, which no file name may",
    );
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[3],
        "REFUSED: a path holds a NUL byte, which no file name may"
    );
    assert_eq!(
        lines[0],
        "REFUSED: `in\0side.txt`: a path holds a NUL byte, which no file name may"
    );
}

#[test]
fn text_that_is_not_utf8_is_refused_and_large_files_round_trip() {
    let case = case(
        "contents",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("latin1.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("empty.txt"))))?
    let big = root.read_only().read_to_string(Path.new("big.txt"))?
    io.println(done(root.write_only().write(Path.new("copy.txt"), big)))?
    let again = root.read_only().read_to_string(Path.new("copy.txt"))?
    let size = again.byte_len()
    if again == big {
        io.println("{size} same")?
    }
    var i = 0
    while i < 300 {
        root.write_only().write(Path.new("many/f{i}"), "")?
        i = i + 1
    }
    let names = root.read_only().list_dir(Path.new("many"))?
    let count = names.len()
    let first = names.at(0)
    let second = names.at(1)
    let last = names.at(299)
    io.println("{count} {first} {second} {last}")?"#,
    )
    .with_file("root/empty.txt", "")
    .with_file("root/many/f0", "")
    .with_setup(|dir| {
        std::fs::write(dir.join("root/latin1.txt"), b"caf\xe9").expect("write");
        let big: String = (0..100_000).map(|i| format!("line {i} é\n")).collect();
        std::fs::write(dir.join("root/big.txt"), big).expect("write");
    });
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        "REFUSED: stream did not contain valid UTF-8\n\n\
         done\n1388890 same\n300 f0 f1 f99\n"
    );
}

#[test]
fn an_entry_name_that_is_not_utf8_fails_the_listing_as_rust_shows_it() {
    let case = case(
        "bad_name",
        r#"    io.println(listed(read.restrict_to("root").list_dir(Path.new("sub"))))?
    io.println(listed(read.restrict_to("root/sub").list_dir(Path.new("."))))?"#,
    )
    .with_setup(|dir| {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"bad\xff\"\xe0\xa0A\xcc\x81'");
        std::fs::write(dir.join("root/sub").join(name), "x").expect("write");
    });
    // The message names the root's canonical path, which is each run's
    // own directory.
    let (interpreted, native) = differential::run(&case);
    assert_eq!(
        interpreted.stdout.replace("/interpreted/", "/native/"),
        native.stdout
    );
    let shown = "\"bad\\xFF\\\"\\xE0\\xA0A\\u{301}'\"";
    for line in native.stdout.lines() {
        assert!(
            line.starts_with(&format!("REFUSED: {shown} in `/"))
                && line.ends_with("/native/root/sub` is not valid UTF-8"),
            "{native:#?}"
        );
    }
}

#[test]
fn no_handle_is_left_open_on_any_path() {
    // Natively only: the program counts its own open descriptors.
    let src = program(
        r#"    let root = fs.restrict_to("root")
    var i = 0
    while i < 2000 {
        let _ = root.read_only().read_to_string(Path.new("inside.txt"))
        let _ = root.read_only().read_to_string(Path.new("out/secret.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub/escape.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub/out/x"))
        let _ = root.read_only().read_to_string(Path.new("leak.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub"))
        let _ = root.read_only().list_dir(Path.new("sub"))
        let _ = root.read_only().list_dir(Path.new("."))
        let _ = root.read_only().list_dir(Path.new("out"))
        let _ = root.write_only().write(Path.new("sub/new.txt"), "x")
        let _ = root.write_only().write(Path.new("leak.txt"), "x")
        let _ = root.write_only().write(Path.new("sub/escape.txt"), "x")
        let _ = root.write_only().create_dir(Path.new("sub/made"))
        let _ = root.write_only().create_dir(Path.new("sub/out/made"))
        let _ = root.write_only().create_dir(Path.new("sub/escape.txt"))
        let narrowed = root.restrict_to(Path.new("sub"))
        let _ = narrowed.restrict_to(Path.new("out"))
        let _ = narrowed.read_only().restrict_to_file(Path.new("nested.txt")).read_to_string()
        let _ = read.restrict_to_file("root/inside.txt").read_to_string()
        let _ = write.restrict_to_file("root/sub/new.txt").write("y")
        let held = Held(dirs: [root.read_only(), read.restrict_to("root/sub")], file: read.restrict_to_file("root/inside.txt"))
        let _ = held.dirs.at(1).read_to_string(Path.new("nested.txt"))
        i = i + 1
    }
    io.println(listed(read.restrict_to("/proc/self/fd").list_dir(Path.new("."))))?"#,
    );
    let src = format!("{src}\ncap struct Held {{ dirs: List<DirRead>, file: FileRead }}\n");
    let case = tree(Case::file("files_handles", &src)).with_setup(|dir| {
        link(dir, "root/out", "../outside");
        link(dir, "root/sub/out", "../../outside");
        link(dir, "root/sub/escape.txt", "../../outside/secret.txt");
        std::fs::hard_link(dir.join("outside/secret.txt"), dir.join("root/leak.txt"))
            .expect("create a hard link");
    });
    let outcome = differential::run_native(&case);
    // stdin, stdout, stderr; `root` (3); and the listing's own: the
    // directory (5, opened through its parent, 4, closed since), its copy
    // for `list_dir` (4) and the open `getdents` reads (6).
    assert_eq!(outcome.stdout, "0,1,2,3,4,5,6\n", "{outcome:#?}");
}

#[test]
fn a_file_or_directory_without_permission_fails_with_the_systems_message() {
    let src = program(
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("locked/x.txt"))))?
    io.println(listed(root.read_only().list_dir(Path.new("locked"))))?
    io.println(listed(read.restrict_to("root/locked").list_dir(Path.new("."))))?
    io.println(done(root.write_only().write(Path.new("locked/x.txt"), "x")))?
    io.println(shown(root.read_only().read_to_string(Path.new("secret.txt"))))?
    io.println(done(root.write_only().write(Path.new("secret.txt"), "x")))?"#,
    );
    let mut seen = Vec::new();
    for side in ["native", "interpreted"] {
        let case = tree(Case::file(&format!("files_permission_{side}"), &src));
        let (exe, dir) = differential::build_native(&case);
        lock(&dir, 0o000);
        let mut command = match side {
            "native" => std::process::Command::new(exe),
            _ => {
                let mut oskac = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"));
                oskac.arg("run").arg("main.osk");
                oskac
            }
        };
        let output = command
            .current_dir(&dir)
            .env_clear()
            .output()
            .expect("runs");
        // Unlocked again, so the tree can be read and removed.
        lock(&dir, 0o700);
        seen.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    assert_eq!(seen[0], seen[1], "the backends differ");
    if !seen[0].contains("Permission denied") {
        // Running as root, which no permission stops: nothing to compare.
        return;
    }
    assert_eq!(
        seen[0],
        "REFUSED: `locked`: Permission denied (os error 13)\n\
         REFUSED: `locked`: Permission denied (os error 13)\n\
         REFUSED: cannot restrict to `root/locked`: `locked`: Permission denied (os error 13)\n\
         REFUSED: `locked`: Permission denied (os error 13)\n\
         REFUSED: `secret.txt`: Permission denied (os error 13)\n\
         REFUSED: `secret.txt`: Permission denied (os error 13)\n"
    );
}

/// Set the mode of `root/locked` and `root/secret.txt` beneath `dir`,
/// making them first.
fn lock(dir: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let locked = dir.join("root/locked");
    let secret = dir.join("root/secret.txt");
    if !locked.exists() {
        std::fs::create_dir(&locked).expect("mkdir");
        std::fs::write(&secret, "s").expect("write");
    }
    for path in [locked, secret] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
}

#[test]
fn the_errno_table_is_rusts_for_every_number() {
    let source = oska_check::STD_RT_SOURCE;
    let head = "fn errno_table() -> String {\n    \"";
    let start = source.find(head).expect("the table") + head.len();
    let end = start + source[start..].find('"').expect("its end");
    let table: Vec<&str> = source[start..end].split('|').collect();
    assert_eq!(table.len(), 133);
    for (index, message) in table.iter().enumerate() {
        let errno = index as i32 + 1;
        let rust = std::io::Error::from_raw_os_error(errno).to_string();
        let expected = if message.is_empty() {
            format!("Unknown error {errno} (os error {errno})")
        } else {
            format!("{message} (os error {errno})")
        };
        assert_eq!(rust, expected, "errno {errno}");
    }
}
