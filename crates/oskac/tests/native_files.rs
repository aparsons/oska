// SPDX-License-Identifier: Apache-2.0
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
//! They run on aarch64 and x86_64 Linux and macOS (tickets #171,
//! #173), and on x86_64 Windows (#178), where a symlink is a Windows
//! symlink and the tests that need a FIFO, a mode, `/` or `/proc` are
//! left out.
//!
//! Symlinks, hard links and FIFOs are planted by a setup function; the
//! harness records a symlink by its target and never follows one, so a
//! write through one that escaped would show in the tree.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

#[path = "common/native_files.rs"]
mod support;

use differential::assert_same;
use std::path::Path;
use support::*;

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
         REFUSED: `missing.txt`: not found\n"
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
        worded(
        "REFUSED: `out` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `sub/out` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `sub/escape.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `gone.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `gone` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `alias.txt` is a symlink, and confined access never follows one\n\
         REFUSED: `alias` is a symlink or not a directory, and confined access never follows a symlink\n\
         REFUSED: `relative.txt` is a symlink, and confined access never follows one\n"
        )
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
        "REFUSED: cannot restrict to `missing`: `missing`: not found"
    );
    assert_eq!(
        lines[16],
        "REFUSED: cannot restrict to `root/inside.txt`: `inside.txt` is a symlink or not a directory, \
         and confined access never follows a symlink"
    );
    assert_eq!(
        lines[18],
        format!(
            "REFUSED: cannot restrict to `root/inside.txt/x`: `{}`: not a directory",
            native("root/inside.txt")
        )
    );
    assert_eq!(
        lines[20],
        "REFUSED: cannot restrict to ``: an empty path names no directory"
    );
    assert!(lines[26].contains("not found"), "{seen}");
    assert_eq!(lines[28], "nested text", "{seen}");
}

#[cfg(unix)]
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
         REFUSED: `pipe`: I/O error\n\
         REFUSED: `sub`: is a directory\n\
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
         REFUSED: `missing`: not found\n\
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
    assert_eq!(lines[14], "REFUSED: `missing`: not found");
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
         REFUSED: `missing.txt`: not found\n"
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
        format!(
            "REFUSED: cannot restrict to `root/nodir/x.txt`: `{}`: not found",
            native("root/nodir")
        )
    );
    assert_eq!(lines[2], "REFUSED: `sub` is not a regular file");
    assert_eq!(
        lines[5],
        "REFUSED: cannot restrict to `root/..`: `root/..` does not end in a file name"
    );
    assert!(!outcome.tree.contains_key("root/nodir/"));
    assert!(!outcome.tree.contains_key("root/x.txt"));
}
