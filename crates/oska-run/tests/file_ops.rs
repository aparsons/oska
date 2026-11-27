// SPDX-License-Identifier: Apache-2.0
//! The file operations beyond read and write (#679, spec #661): `exists`
//! and `metadata` on a directory capability and `metadata` on a file
//! capability, `append` on a directory and a file capability, and
//! `remove_file`, `remove_dir` and `rename` on a directory capability.
//! Each is confined exactly like `read_to_string` and `write` (ADR 0008).
//!
//! The API tests run Oska programs on [`FakeHost`], on every OS; each is
//! also a differential test of the native backend on Linux (`common`).
//! The attack tests run them on [`oska_run::RealHost`] against a real
//! temporary directory, with real symlinks and hard links, and each is red
//! without the refusal it checks: a program that is not refused would
//! change or remove the file outside the root.

mod common;

use common::{run_differential, run_on, TempTree};
use oska_run::FakeHost;

/// The helpers the programs below add to `common::SHOWN`.
const EXTRA: &str = r#"
fn flag(result: Result<Bool, IoError>) -> Untrusted<String> {
    match result {
        Ok(there) => if there { "true" } else { "false" }
        Err(e) => refused(e)
    }
}

fn described(result: Result<Metadata, IoError>) -> Untrusted<String> {
    match result {
        Ok(m) => {
            let size = m.size
            match m.kind {
                FileKind.File => "file {size}"
                FileKind.Directory => "directory {size}"
                FileKind.Other => "other {size}"
            }
        }
        Err(e) => refused(e)
    }
}
"#;

/// The world every API test starts from.
fn world() -> FakeHost {
    FakeHost::new()
        .with_file("root/a.txt", "alpha")
        .with_file("root/sub/b.txt", "bee")
        .with_dir("root/empty")
        .with_file("outside/secret.txt", "secret")
}

/// What `body` printed on `host`, in a program whose `main` has `io`,
/// `fs`, `read` (a `DirRead` on `/root`) and `write` (a `DirWrite` on it).
fn printed(host: &mut FakeHost, body: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n\
         \x20   let read = fs.read_only().restrict_to(\"/root\")\n\
         \x20   let write = fs.write_only().restrict_to(\"/root\")\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n{}{EXTRA}",
        common::SHOWN
    );
    let outcome = run_differential(host, &src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// The line `io.println(...)` of each of `lines`.
fn lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| format!("    io.println({line})?\n"))
        .collect()
}

const ESCAPES: &str = "REFUSED: `../outside/secret.txt` leads outside the capability's root";
const ABSOLUTE: &str = "REFUSED: `/outside/secret.txt` is absolute; \
                        a directory capability takes paths relative to its root";

// ---- exists and metadata ---------------------------------------------------------------

#[test]
fn exists_says_whether_a_name_is_there() {
    let out = printed(
        &mut world(),
        &lines(&[
            "flag(read.exists(Path.new(\"a.txt\")))",
            "flag(read.exists(Path.new(\"sub\")))",
            "flag(read.exists(Path.new(\"empty\")))",
            "flag(read.exists(Path.new(\".\")))",
            "flag(read.exists(Path.new(\"sub/../a.txt\")))",
            "flag(read.exists(Path.new(\"missing.txt\")))",
            "flag(read.exists(Path.new(\"sub/missing.txt\")))",
        ]),
    );
    assert_eq!(out, "true\ntrue\ntrue\ntrue\ntrue\nfalse\nfalse\n");
}

#[test]
fn exists_refuses_a_path_that_leaves_the_root_rather_than_say_false() {
    let out = printed(
        &mut world(),
        &lines(&[
            "flag(read.exists(Path.new(\"../outside/secret.txt\")))",
            "flag(read.exists(Path.new(\"/outside/secret.txt\")))",
            "flag(read.exists(Path.new(\"\")))",
        ]),
    );
    assert_eq!(
        out,
        format!("{ESCAPES}\n{ABSOLUTE}\nREFUSED: an empty path names no file\n")
    );
}

#[test]
fn metadata_gives_kind_and_size_only() {
    let out = printed(
        &mut world(),
        &lines(&[
            "described(read.metadata(Path.new(\"a.txt\")))",
            "described(read.metadata(Path.new(\"sub\")))",
            "described(read.metadata(Path.new(\".\")))",
            "described(read.metadata(Path.new(\"missing.txt\")))",
            "described(read.metadata(Path.new(\"../outside/secret.txt\")))",
            "described(read.metadata(Path.new(\"/outside/secret.txt\")))",
        ]),
    );
    assert_eq!(
        out,
        format!(
            "file 5\ndirectory 0\ndirectory 0\n\
             REFUSED: `missing.txt`: not found\n{ESCAPES}\n{ABSOLUTE}\n"
        )
    );
}

#[test]
fn a_file_capability_gives_its_files_metadata() {
    let out = printed(
        &mut world(),
        &lines(&[
            "described(read.restrict_to_file(Path.new(\"a.txt\")).metadata())",
            "described(read.restrict_to_file(Path.new(\"missing.txt\")).metadata())",
        ]),
    );
    assert_eq!(out, "file 5\nREFUSED: `missing.txt`: not found\n");
}

// ---- append ----------------------------------------------------------------------------

#[test]
fn append_adds_to_the_end_and_creates_a_missing_file() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.append(Path.new(\"a.txt\"), \" and more\"))",
            "done(write.append(Path.new(\"a.txt\"), \"!\"))",
            "done(write.append(Path.new(\"log.txt\"), \"one\\n\"))",
            "done(write.append(Path.new(\"log.txt\"), \"two\\n\"))",
            "done(write.append(Path.new(\"sub/c.txt\"), \"c\"))",
            "shown(read.read_to_string(Path.new(\"a.txt\")))",
        ]),
    );
    assert_eq!(out, "done\ndone\ndone\ndone\ndone\nalpha and more!\n");
}

#[test]
fn append_through_a_file_capability_adds_to_its_one_file() {
    let out = printed(
        &mut world(),
        &lines(&[
            "done(write.restrict_to_file(Path.new(\"a.txt\")).append(\"+1\"))",
            "done(write.restrict_to_file(Path.new(\"a.txt\")).append(\"+2\"))",
            "done(write.restrict_to_file(Path.new(\"new.txt\")).append(\"n\"))",
            "shown(read.read_to_string(Path.new(\"a.txt\")))",
            "shown(read.read_to_string(Path.new(\"new.txt\")))",
        ]),
    );
    assert_eq!(out, "done\ndone\ndone\nalpha+1+2\nn\n");
}

#[test]
fn append_refuses_a_directory_a_missing_parent_and_a_path_that_leaves_the_root() {
    let out = printed(
        &mut world(),
        &lines(&[
            "done(write.append(Path.new(\"sub\"), \"x\"))",
            "done(write.append(Path.new(\"../outside/secret.txt\"), \"x\"))",
            "done(write.append(Path.new(\"/outside/secret.txt\"), \"x\"))",
            "done(write.append(Path.new(\"sub/\"), \"x\"))",
        ]),
    );
    assert_eq!(
        out,
        format!(
            "REFUSED: `sub` is not a regular file\n{ESCAPES}\n{ABSOLUTE}\n\
             REFUSED: `sub/` does not end in a file name; \
             a write, a new directory or a file needs one\n"
        )
    );
}

// ---- remove_file -----------------------------------------------------------------------

#[test]
fn remove_file_removes_one_file_and_keeps_its_directory() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.remove_file(Path.new(\"sub/b.txt\")))",
            "flag(read.exists(Path.new(\"sub/b.txt\")))",
            "flag(read.exists(Path.new(\"sub\")))",
            "flag(read.exists(Path.new(\"a.txt\")))",
            "done(write.remove_file(Path.new(\"sub/b.txt\")))",
        ]),
    );
    assert_eq!(
        out,
        "done\nfalse\ntrue\ntrue\nREFUSED: `sub/b.txt`: not found\n"
    );
    assert_eq!(host.file("root/a.txt"), Some("alpha"));
}

#[test]
fn remove_file_refuses_a_directory_and_a_path_that_leaves_the_root() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.remove_file(Path.new(\"sub\")))",
            "done(write.remove_file(Path.new(\"empty\")))",
            "done(write.remove_file(Path.new(\"../outside/secret.txt\")))",
            "done(write.remove_file(Path.new(\"/outside/secret.txt\")))",
            "done(write.remove_file(Path.new(\".\")))",
            "done(write.remove_file(Path.new(\"sub/..\")))",
        ]),
    );
    let not_a_file = |name: &str| format!("REFUSED: `{name}` is not a regular file\n");
    let no_name = |path: &str| {
        format!(
            "REFUSED: `{path}` does not end in a file name; \
             a write, a new directory or a file needs one\n"
        )
    };
    assert_eq!(
        out,
        format!(
            "{}{}{ESCAPES}\n{ABSOLUTE}\n{}{}",
            not_a_file("sub"),
            not_a_file("empty"),
            no_name("."),
            no_name("sub/..")
        )
    );
    assert_eq!(host.file("outside/secret.txt"), Some("secret"));
    assert_eq!(host.file("root/sub/b.txt"), Some("bee"));
}

// ---- remove_dir ------------------------------------------------------------------------

#[test]
fn remove_dir_removes_an_empty_directory_only() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.remove_dir(Path.new(\"empty\")))",
            "flag(read.exists(Path.new(\"empty\")))",
            "done(write.remove_dir(Path.new(\"sub\")))",
            "done(write.remove_dir(Path.new(\"a.txt\")))",
            "done(write.remove_dir(Path.new(\"empty\")))",
            "done(write.remove_dir(Path.new(\"../outside\")))",
            "done(write.remove_dir(Path.new(\"/outside\")))",
        ]),
    );
    assert_eq!(
        out,
        "done\nfalse\nREFUSED: `sub`: directory not empty\n\
         REFUSED: `a.txt` is a symlink or not a directory, \
         and confined access never follows a symlink\n\
         REFUSED: `empty`: not found\n\
         REFUSED: `../outside` leads outside the capability's root\n\
         REFUSED: `/outside` is absolute; \
         a directory capability takes paths relative to its root\n"
    );
    assert_eq!(
        host.file("root/sub/b.txt"),
        Some("bee"),
        "nothing recursive"
    );
    assert_eq!(host.file("outside/secret.txt"), Some("secret"));
}

#[test]
fn a_directory_emptied_by_remove_file_can_then_be_removed() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.remove_file(Path.new(\"sub/b.txt\")))",
            "done(write.remove_dir(Path.new(\"sub\")))",
            "flag(read.exists(Path.new(\"sub\")))",
        ]),
    );
    assert_eq!(out, "done\ndone\nfalse\n");
}

// ---- rename ----------------------------------------------------------------------------

#[test]
fn rename_moves_a_file_and_replaces_one() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.rename(Path.new(\"a.txt\"), Path.new(\"moved.txt\")))",
            "flag(read.exists(Path.new(\"a.txt\")))",
            "shown(read.read_to_string(Path.new(\"moved.txt\")))",
            "done(write.rename(Path.new(\"moved.txt\"), Path.new(\"sub/b.txt\")))",
            "shown(read.read_to_string(Path.new(\"sub/b.txt\")))",
            "done(write.rename(Path.new(\"sub/b.txt\"), Path.new(\"sub/./b.txt\")))",
            "shown(read.read_to_string(Path.new(\"sub/b.txt\")))",
        ]),
    );
    assert_eq!(out, "done\nfalse\nalpha\ndone\nalpha\ndone\nalpha\n");
}

#[test]
fn rename_refuses_what_it_cannot_move_or_replace() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.rename(Path.new(\"missing.txt\"), Path.new(\"x.txt\")))",
            "done(write.rename(Path.new(\"sub\"), Path.new(\"x\")))",
            "done(write.rename(Path.new(\"a.txt\"), Path.new(\"sub\")))",
        ]),
    );
    assert!(
        out.starts_with(
            "REFUSED: `missing.txt`: not found\n\
             REFUSED: `sub` is not a regular file\n\
             REFUSED: `sub` is not a regular file\n"
        ),
        "{out}"
    );
    assert_eq!(out.lines().count(), 3, "{out}");
    assert_eq!(host.file("root/a.txt"), Some("alpha"), "nothing moved");
}

#[test]
fn rename_stays_inside_the_capability_whichever_name_leaves() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.rename(Path.new(\"a.txt\"), Path.new(\"../outside/a.txt\")))",
            "done(write.rename(Path.new(\"a.txt\"), Path.new(\"/outside/a.txt\")))",
            "done(write.rename(Path.new(\"../outside/secret.txt\"), Path.new(\"s.txt\")))",
            "done(write.rename(Path.new(\"/outside/secret.txt\"), Path.new(\"s.txt\")))",
        ]),
    );
    assert_eq!(
        out,
        "REFUSED: `../outside/a.txt` leads outside the capability's root\n\
         REFUSED: `/outside/a.txt` is absolute; \
         a directory capability takes paths relative to its root\n\
         REFUSED: `../outside/secret.txt` leads outside the capability's root\n\
         REFUSED: `/outside/secret.txt` is absolute; \
         a directory capability takes paths relative to its root\n"
    );
    assert_eq!(host.file("root/a.txt"), Some("alpha"));
    assert_eq!(host.file("outside/secret.txt"), Some("secret"));
    assert_eq!(host.file("outside/a.txt"), None);
}

// ---- Who may do what -------------------------------------------------------------------

#[test]
fn a_read_capability_cannot_write_and_a_write_capability_cannot_read() {
    let src = |call: &str| {
        format!(
            "fn main(fs: Fs) -> Result<(), IoError> {{\n    \
             let read = fs.read_only().restrict_to(\"/root\")\n    \
             let write = fs.write_only().restrict_to(\"/root\")\n    {call}\n    Ok(())\n}}\n"
        )
    };
    for call in [
        "read.append(Path.new(\"a\"), \"x\")?",
        "read.remove_file(Path.new(\"a\"))?",
        "read.remove_dir(Path.new(\"a\"))?",
        "read.rename(Path.new(\"a\"), Path.new(\"b\"))?",
        "let found = write.exists(Path.new(\"a\"))?",
        "let found = write.metadata(Path.new(\"a\"))?",
    ] {
        let outcome = run_on(FakeHost::new(), &src(call), &[]);
        assert!(
            outcome.diagnostics.iter().any(|d| d.code == "E0228"),
            "{call} should not check: {outcome:#?}"
        );
    }
}

// ---- Attacks on a real directory (the files outside the root stay as they are) ---------

/// Every attack below, red without the refusal it checks.
#[cfg(any(unix, windows))]
mod real {
    use super::TempTree;
    use crate::common::symlink;

    /// A tree with `root/inside.txt`, `root/sub/`, a file `outside/secret.txt`,
    /// and `root/link.txt` a symlink to it.
    fn linked(test: &str) -> TempTree {
        let tree = TempTree::new(&format!("fileops-{test}"));
        tree.write("root/inside.txt", "inside");
        tree.write("root/sub/nested.txt", "nested");
        tree.write("outside/secret.txt", "outside text");
        symlink(
            &tree.path("outside/secret.txt"),
            &tree.path("root/link.txt"),
        );
        tree
    }

    /// A tree whose `root/leak.txt` is a hard link to `outside/secret.txt`.
    fn hard_linked(test: &str) -> TempTree {
        let tree = TempTree::new(&format!("fileops-{test}"));
        tree.write("root/inside.txt", "inside");
        tree.write("outside/secret.txt", "outside text");
        std::fs::hard_link(tree.path("outside/secret.txt"), tree.path("root/leak.txt"))
            .expect("create a hard link");
        tree
    }

    const SYMLINK: &str = "is a symlink, and confined access never follows one";
    const HARD_LINK: &str = "has more than one hard link, and confined access never opens one";

    fn refused(what: &str, why: &str) -> String {
        format!("REFUSED: `{what}` {why}")
    }

    fn intact(tree: &TempTree) {
        assert_eq!(tree.file("outside/secret.txt"), "outside text");
    }

    #[test]
    fn dot_dot_and_absolute_paths_reach_nothing_outside_for_every_operation() {
        let tree = linked("escape");
        let outside = crate::common::literal(tree.path("outside/secret.txt"));
        let body = format!(
            "    say(flag(read.exists(Path.new(\"../outside/secret.txt\"))))\n    \
             say(described(read.metadata(Path.new(\"../outside/secret.txt\"))))\n    \
             say(done(write.append(Path.new(\"../outside/secret.txt\"), \"x\")))\n    \
             say(done(write.remove_file(Path.new(\"../outside/secret.txt\"))))\n    \
             say(done(write.remove_file(Path.new(\"{outside}\"))))\n    \
             say(done(write.remove_dir(Path.new(\"../outside\"))))\n    \
             say(done(write.rename(Path.new(\"../outside/secret.txt\"), Path.new(\"in.txt\"))))\n    \
             say(done(write.rename(Path.new(\"inside.txt\"), Path.new(\"../outside/secret.txt\"))))\n    \
             say(done(write.rename(Path.new(\"inside.txt\"), Path.new(\"{outside}\"))))"
        );
        let seen = saw(&tree, &body);
        let leaves = |path: &str| format!("REFUSED: `{path}` leads outside the capability's root");
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines.len(), 9, "{seen}");
        for (at, path) in [
            (0, "../outside/secret.txt"),
            (1, "../outside/secret.txt"),
            (2, "../outside/secret.txt"),
            (3, "../outside/secret.txt"),
            (5, "../outside"),
            (6, "../outside/secret.txt"),
            (7, "../outside/secret.txt"),
        ] {
            assert_eq!(lines[at], leaves(path), "{seen}");
        }
        for at in [4, 8] {
            assert!(lines[at].contains("is absolute"), "{seen}");
        }
        intact(&tree);
        assert_eq!(tree.file("root/inside.txt"), "inside");
    }

    #[test]
    fn remove_file_refuses_a_symlink_and_leaves_both_ends() {
        let tree = linked("remove-link");
        let seen = saw(
            &tree,
            "    say(done(write.remove_file(Path.new(\"link.txt\"))))",
        );
        assert_eq!(seen, refused("link.txt", SYMLINK));
        intact(&tree);
        assert!(
            std::fs::symlink_metadata(tree.path("root/link.txt")).is_ok(),
            "the link is still there"
        );
    }

    #[test]
    fn remove_file_refuses_a_hard_link() {
        let tree = hard_linked("remove-hard");
        let seen = saw(
            &tree,
            "    say(done(write.remove_file(Path.new(\"leak.txt\"))))",
        );
        assert_eq!(seen, refused("leak.txt", HARD_LINK));
        intact(&tree);
        assert!(tree.path("root/leak.txt").exists());
    }

    #[test]
    fn remove_file_and_remove_dir_refuse_a_symlinked_directory_on_the_way() {
        let tree = linked("remove-dir-link");
        symlink(&tree.path("outside"), &tree.path("root/out"));
        let seen = saw(
            &tree,
            "    say(done(write.remove_file(Path.new(\"out/secret.txt\"))))\n    \
             say(done(write.remove_dir(Path.new(\"out\"))))",
        );
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines.len(), 2, "{seen}");
        assert!(lines[0].starts_with("REFUSED: `out`"), "{seen}");
        assert!(lines[1].starts_with("REFUSED: `out`"), "{seen}");
        intact(&tree);
    }

    #[test]
    fn rename_refuses_a_symlink_as_source_and_as_destination() {
        let tree = linked("rename-link");
        let seen = saw(
            &tree,
            "    say(done(write.rename(Path.new(\"link.txt\"), Path.new(\"moved.txt\"))))\n    \
             say(done(write.rename(Path.new(\"inside.txt\"), Path.new(\"link.txt\"))))",
        );
        assert_eq!(
            seen,
            format!(
                "{}\n{}",
                refused("link.txt", SYMLINK),
                refused("link.txt", SYMLINK)
            )
        );
        intact(&tree);
        assert_eq!(tree.file("root/inside.txt"), "inside");
        assert!(!tree.path("root/moved.txt").exists());
    }

    #[test]
    fn rename_refuses_a_hard_link_as_source_and_as_destination() {
        let tree = hard_linked("rename-hard");
        let seen = saw(
            &tree,
            "    say(done(write.rename(Path.new(\"leak.txt\"), Path.new(\"moved.txt\"))))\n    \
             say(done(write.rename(Path.new(\"inside.txt\"), Path.new(\"leak.txt\"))))",
        );
        assert_eq!(
            seen,
            format!(
                "{}\n{}",
                refused("leak.txt", HARD_LINK),
                refused("leak.txt", HARD_LINK)
            )
        );
        intact(&tree);
        assert_eq!(tree.file("root/inside.txt"), "inside");
        assert!(!tree.path("root/moved.txt").exists());
    }

    #[test]
    fn append_and_metadata_refuse_a_symlink_and_a_hard_link() {
        let linked = linked("append-link");
        let seen = saw(
            &linked,
            "    say(done(write.append(Path.new(\"link.txt\"), \"x\")))\n    \
             say(described(read.metadata(Path.new(\"link.txt\"))))\n    \
             say(flag(read.exists(Path.new(\"link.txt\"))))",
        );
        let symlink = refused("link.txt", SYMLINK);
        assert_eq!(seen, format!("{symlink}\n{symlink}\n{symlink}"));
        intact(&linked);

        let hard = hard_linked("append-hard");
        let seen = saw(
            &hard,
            "    say(done(write.append(Path.new(\"leak.txt\"), \"x\")))\n    \
             say(described(read.metadata(Path.new(\"leak.txt\"))))\n    \
             say(flag(read.exists(Path.new(\"leak.txt\"))))",
        );
        let hard_link = refused("leak.txt", HARD_LINK);
        assert_eq!(seen, format!("{hard_link}\n{hard_link}\ntrue"));
        intact(&hard);
    }

    #[test]
    fn a_file_capability_refuses_a_symlink_and_a_hard_link_for_append_and_metadata() {
        let tree = linked("file-cap");
        let seen = saw(
            &tree,
            "    let link = fs.write_only().restrict_to_file(\"{root}/link.txt\")\n    \
             say(done(link.append(\"x\")))\n    \
             let seen_link = fs.read_only().restrict_to_file(\"{root}/link.txt\")\n    \
             say(described(seen_link.metadata()))",
        );
        let symlink = refused("link.txt", SYMLINK);
        assert_eq!(seen, format!("{symlink}\n{symlink}"));
        intact(&tree);
    }

    #[test]
    fn a_new_file_is_appended_and_an_ordinary_one_removed_and_renamed_on_a_real_directory() {
        let tree = linked("works");
        let seen = saw(
            &tree,
            "    say(done(write.append(Path.new(\"log.txt\"), \"one\\n\")))\n    \
             say(done(write.append(Path.new(\"log.txt\"), \"two\\n\")))\n    \
             say(described(read.metadata(Path.new(\"log.txt\"))))\n    \
             say(described(read.metadata(Path.new(\"sub\"))))\n    \
             say(done(write.rename(Path.new(\"log.txt\"), Path.new(\"sub/log.txt\"))))\n    \
             say(done(write.rename(Path.new(\"inside.txt\"), Path.new(\"sub/log.txt\"))))\n    \
             say(done(write.remove_file(Path.new(\"sub/log.txt\"))))\n    \
             say(done(write.remove_dir(Path.new(\"sub\"))))\n    \
             say(done(write.remove_file(Path.new(\"sub/nested.txt\"))))\n    \
             say(done(write.remove_dir(Path.new(\"sub\"))))",
        );
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(
            lines[..8],
            [
                "done",
                "done",
                "file 8",
                // A directory's size is not defined, so it is 0.
                "directory 0",
                "done",
                "done",
                "done",
                "REFUSED: `sub`: directory not empty",
            ]
        );
        assert_eq!(lines[8], "done");
        assert_eq!(lines[9], "done");
        assert!(!tree.path("root/sub").exists());
        assert!(!tree.path("root/inside.txt").exists());
        intact(&tree);
    }

    /// What `body` printed, with the extra helpers in scope.
    fn saw(tree: &TempTree, body: &str) -> String {
        tree.saw_with(body, super::EXTRA)
    }
}

/// Windows reserved names are refused by every new operation, in either
/// name of a rename (the names are ordinary files elsewhere).
#[cfg(windows)]
mod windows_reserved {
    use super::TempTree;

    #[test]
    fn a_device_name_or_a_stream_is_refused_for_every_operation() {
        let tree = TempTree::new("fileops-winnames");
        tree.write("root/ok.txt", "ok");
        for name in ["CON", "nul.txt", "COM1", "aux", "lpt9.log", "ok.txt:stream"] {
            let body = format!(
                "    say(flag(read.exists(Path.new(\"{name}\"))))\n    \
                 say(described(read.metadata(Path.new(\"{name}\"))))\n    \
                 say(done(write.append(Path.new(\"{name}\"), \"x\")))\n    \
                 say(done(write.remove_file(Path.new(\"{name}\"))))\n    \
                 say(done(write.remove_dir(Path.new(\"{name}\"))))\n    \
                 say(done(write.rename(Path.new(\"ok.txt\"), Path.new(\"{name}\"))))\n    \
                 say(done(write.rename(Path.new(\"{name}\"), Path.new(\"ok2.txt\"))))"
            );
            let seen = tree.saw_with(&body, super::EXTRA);
            let lines: Vec<&str> = seen.lines().collect();
            assert_eq!(lines.len(), 7, "{name}: {seen}");
            for line in lines {
                assert!(
                    line.starts_with(&format!("REFUSED: `{name}` ")),
                    "{name}: {seen}"
                );
                assert!(
                    line.ends_with("and confined access never opens one"),
                    "{line}"
                );
            }
            assert_eq!(tree.file("root/ok.txt"), "ok");
        }
    }
}
