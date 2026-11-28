// SPDX-License-Identifier: Apache-2.0
//! `FileWrite.sync` and `DirWrite.sync` (#845, spec #842, ADR 0008): flush
//! a file's data and metadata to stable storage, and flush a directory so
//! a rename, a new file or a removal in it is durable. They are the two
//! `sync` steps of the operation order in `formal/tla/store-log.md`.
//!
//! Both are confined like `append` and `rename`: they exist on the write
//! capabilities only, `sync` on a file never creates it, follows a symlink
//! or opens a hard link, and `sync` on a directory reaches only the
//! directory the capability is rooted at, however it was narrowed.
//!
//! The API tests run Oska programs on [`FakeHost`], on every OS; each is
//! also a differential test of the native backend on Linux (`common`). The
//! fake host loses nothing in a crash, so a sync there changes nothing and
//! is recorded for the fault-injecting host of a later ticket (#847). The
//! attack tests run on [`oska_run::RealHost`] against a real temporary
//! directory, with real symlinks and hard links.

mod common;

use common::{run_on, TempTree};
use oska_run::{FakeHost, Host};

/// The world every API test starts from.
fn world() -> FakeHost {
    FakeHost::new()
        .with_file("root/a.txt", "alpha")
        .with_file("root/sub/b.txt", "bee")
        .with_dir("root/empty")
        .with_file("outside/secret.txt", "secret")
}

/// What `body` printed on `host`, in a program whose `main` has `io`,
/// `fs` and `write` (a `DirWrite` on `/root`).
fn printed(host: &mut FakeHost, body: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n\
         \x20   let write = fs.write_only().restrict_to(\"/root\")\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n{}",
        common::SHOWN
    );
    let outcome = common::run_differential(host, &src, &[]);
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

// ---- The methods -----------------------------------------------------------------------

#[test]
fn a_written_file_can_be_synced_and_keeps_what_it_holds() {
    let mut host = world();
    let out = printed(
        &mut host,
        "    let file = fs.write_only().restrict_to_file(\"/root/new.txt\")\n    \
         io.println(done(file.write(\"one\")))?\n    \
         io.println(done(file.append(\"two\")))?\n    \
         io.println(done(file.sync()))?\n",
    );
    assert_eq!(out, "done\ndone\ndone\n");
    assert_eq!(
        host.file("root/new.txt"),
        Some("onetwo"),
        "sync changes nothing"
    );
}

#[test]
fn a_directory_can_be_synced_after_a_rename_a_new_file_and_a_removal() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.rename(Path.new(\"a.txt\"), Path.new(\"sub/a.txt\")))",
            "done(write.sync())",
            "done(write.write(Path.new(\"new.txt\"), \"n\"))",
            "done(write.sync())",
            "done(write.remove_file(Path.new(\"new.txt\")))",
            "done(write.sync())",
        ]),
    );
    assert_eq!(out, "done\n".repeat(6));
    assert_eq!(host.file("root/sub/a.txt"), Some("alpha"));
    assert_eq!(host.file("root/a.txt"), None);
    assert_eq!(host.file("root/new.txt"), None);
}

#[test]
fn a_directory_capability_narrowed_to_a_subdirectory_syncs_that_directory() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.restrict_to(Path.new(\"sub\")).sync())",
            "done(write.restrict_to(Path.new(\"empty\")).sync())",
        ]),
    );
    assert_eq!(out, "done\ndone\n");
    assert_eq!(
        host.syncs(),
        vec!["dir /root/sub".to_string(), "dir /root/empty".to_string()],
    );
}

#[test]
fn a_file_capability_from_a_directory_syncs_its_file() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.restrict_to_file(Path.new(\"sub/b.txt\")).sync())",
            "done(write.restrict_to_file(Path.new(\"a.txt\")).sync())",
        ]),
    );
    assert_eq!(out, "done\ndone\n");
    assert_eq!(
        host.syncs(),
        vec![
            "file /root/sub/b.txt".to_string(),
            "file /root/a.txt".to_string()
        ]
    );
}

#[test]
fn a_sync_is_recorded_in_order_for_a_host_that_models_a_crash() {
    let mut host = world();
    printed(
        &mut host,
        &lines(&[
            "done(write.restrict_to_file(Path.new(\"a.txt\")).sync())",
            "done(write.sync())",
        ]),
    );
    assert_eq!(
        host.syncs(),
        vec!["file /root/a.txt".to_string(), "dir /root".to_string()]
    );
}

#[test]
fn syncing_a_file_that_is_not_there_is_an_error_and_creates_nothing() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.restrict_to_file(Path.new(\"missing.txt\")).sync())",
            "done(write.restrict_to_file(Path.new(\"empty/missing.txt\")).sync())",
        ]),
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    for line in &lines {
        assert!(line.starts_with("REFUSED: "), "{out}");
    }
    assert_eq!(lines[0], "REFUSED: `missing.txt`: not found");
    assert_eq!(host.file("root/missing.txt"), None, "a sync never creates");
    assert_eq!(host.file("root/empty/missing.txt"), None);
    assert!(host.syncs().is_empty(), "a failed sync is not recorded");
}

#[test]
fn syncing_a_directory_through_a_file_capability_is_refused() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&["done(write.restrict_to_file(Path.new(\"sub\")).sync())"]),
    );
    assert_eq!(out, "REFUSED: `sub` is not a regular file\n");
    assert!(host.syncs().is_empty());
}

#[test]
fn a_path_that_leaves_the_root_is_refused_when_narrowing_for_sync() {
    let mut host = world();
    let out = printed(
        &mut host,
        &lines(&[
            "done(write.restrict_to(Path.new(\"..\")).sync())",
            "done(write.restrict_to_file(Path.new(\"../outside/secret.txt\")).sync())",
            "done(write.restrict_to_file(Path.new(\"/outside/secret.txt\")).sync())",
        ]),
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    for line in lines {
        assert!(line.starts_with("REFUSED: "), "{out}");
    }
    assert!(host.syncs().is_empty());
}

// ---- Who may do what -------------------------------------------------------------------

#[test]
fn only_the_write_capabilities_have_sync() {
    let src = |call: &str| {
        format!(
            "fn main(fs: Fs) -> Result<(), IoError> {{\n    \
             let read = fs.read_only().restrict_to(\"/root\")\n    \
             let write = fs.write_only().restrict_to(\"/root\")\n    \
             let file_read = fs.read_only().restrict_to_file(\"/root/a.txt\")\n    {call}\n    Ok(())\n}}\n"
        )
    };
    for call in [
        "read.sync()?",
        "file_read.sync()?",
        "fs.sync()?",
        "fs.read_only().sync()?",
        "fs.write_only().sync()?",
    ] {
        let outcome = run_on(world(), &src(call), &[]);
        assert!(
            !outcome.diagnostics.is_empty(),
            "{call} should not check: {outcome:#?}"
        );
    }
    // The write capabilities have it.
    for call in [
        "write.sync()?",
        "fs.write_only().restrict_to_file(\"/root/a.txt\").sync()?",
    ] {
        let outcome = run_on(world(), &src(call), &[]);
        assert!(
            outcome.diagnostics.is_empty() && outcome.panic.is_none(),
            "{call} should run: {outcome:#?}"
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
        let tree = TempTree::new(&format!("filesync-{test}"));
        tree.write("root/inside.txt", "inside");
        tree.write("root/sub/nested.txt", "nested");
        tree.write("outside/secret.txt", "outside text");
        symlink(
            &tree.path("outside/secret.txt"),
            &tree.path("root/link.txt"),
        );
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
    fn a_real_file_and_a_real_directory_sync_and_keep_their_contents() {
        let tree = linked("works");
        let seen = tree.saw(
            "    let file = fs.write_only().restrict_to_file(\"{root}/log.txt\")\n    \
             say(done(file.write(\"one\\n\")))\n    \
             say(done(file.append(\"two\\n\")))\n    \
             say(done(file.sync()))\n    \
             say(done(write.rename(Path.new(\"log.txt\"), Path.new(\"sub/log.txt\"))))\n    \
             say(done(write.sync()))\n    \
             say(done(write.restrict_to(Path.new(\"sub\")).sync()))\n    \
             say(done(write.restrict_to_file(Path.new(\"inside.txt\")).sync()))",
        );
        assert_eq!(seen, "done\n".repeat(7).trim_end());
        assert_eq!(tree.file("root/sub/log.txt"), "one\ntwo\n");
        assert_eq!(tree.file("root/inside.txt"), "inside");
        assert!(!tree.path("root/log.txt").exists());
        intact(&tree);
    }

    #[test]
    fn syncing_a_missing_name_creates_nothing_on_a_real_directory() {
        let tree = linked("missing");
        let seen = tree.saw(
            "    say(done(write.restrict_to_file(Path.new(\"missing.txt\")).sync()))\n    \
             say(done(write.restrict_to(Path.new(\"missing\")).sync()))",
        );
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines.len(), 2, "{seen}");
        for line in lines {
            assert!(line.starts_with("REFUSED: "), "{seen}");
        }
        assert!(!tree.path("root/missing.txt").exists());
        assert!(!tree.path("root/missing").exists());
        intact(&tree);
    }

    #[test]
    fn a_file_capability_refuses_to_sync_a_symlink_and_a_hard_link() {
        let tree = linked("links");
        std::fs::hard_link(tree.path("outside/secret.txt"), tree.path("root/leak.txt"))
            .expect("create a hard link");
        let seen = tree.saw(
            "    say(done(fs.write_only().restrict_to_file(\"{root}/link.txt\").sync()))\n    \
             say(done(write.restrict_to_file(Path.new(\"link.txt\")).sync()))\n    \
             say(done(write.restrict_to_file(Path.new(\"leak.txt\")).sync()))",
        );
        let symlink = refused("link.txt", SYMLINK);
        let hard = refused("leak.txt", HARD_LINK);
        assert_eq!(seen, format!("{symlink}\n{symlink}\n{hard}"));
        intact(&tree);
    }

    #[test]
    fn a_directory_that_is_a_symlink_is_not_synced_through() {
        let tree = linked("dirlink");
        symlink(&tree.path("outside"), &tree.path("root/outside-link"));
        let seen = tree.saw(
            "    say(done(write.restrict_to(Path.new(\"outside-link\")).sync()))\n    \
             say(done(write.restrict_to_file(Path.new(\"outside-link/secret.txt\")).sync()))",
        );
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines.len(), 2, "{seen}");
        for line in lines {
            assert!(line.starts_with("REFUSED: "), "{seen}");
            assert!(line.contains("symlink"), "{seen}");
        }
        intact(&tree);
    }

    #[test]
    fn a_path_outside_the_root_is_refused_on_a_real_directory() {
        let tree = linked("escape");
        let outside = crate::common::literal(tree.path("outside/secret.txt"));
        let body = format!(
            "    say(done(write.restrict_to_file(Path.new(\"../outside/secret.txt\")).sync()))\n    \
             say(done(write.restrict_to_file(Path.new(\"{outside}\")).sync()))\n    \
             say(done(write.restrict_to(Path.new(\"..\")).sync()))"
        );
        let seen = tree.saw(&body);
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines.len(), 3, "{seen}");
        assert!(
            lines[0].ends_with("`../outside/secret.txt` leads outside the capability's root"),
            "{seen}"
        );
        for line in lines {
            assert!(line.starts_with("REFUSED: "), "{seen}");
        }
        intact(&tree);
    }

    #[test]
    fn a_directory_given_as_a_file_is_refused_on_a_real_directory() {
        let tree = linked("dirfile");
        let seen = tree.saw("    say(done(write.restrict_to_file(Path.new(\"sub\")).sync()))");
        assert_eq!(seen, "REFUSED: `sub` is not a regular file");
    }
}
