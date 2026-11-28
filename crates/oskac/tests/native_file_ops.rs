// SPDX-License-Identifier: Apache-2.0
//! The file operations beyond read and write on every backend (#679,
//! spec #661), through the differential harness: `exists`, `metadata`,
//! `append`, `remove_file`, `remove_dir` and `rename`, with the same
//! refusals as the interpreter's real-OS suite
//! (`oska-run/tests/file_ops.rs`): `..`, an absolute path, a symlink and a
//! hard link as the target of each. The harness records a symlink by its
//! target and never follows one, so a removal or write through one that
//! escaped would show in the tree it compares.
//!
//! Every case starts from the same tree, relative to the working
//! directory:
//!
//! ```text
//! root/inside.txt         "inside text"
//! root/sub/nested.txt     "nested text"
//! outside/secret.txt      "outside text"
//! ```

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case, Outcome};
use std::path::Path;

/// The helpers every program has: what an operation gave, as one line.
const SHOWN: &str = r#"
fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

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

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

fn case(name: &str, body: &str, setup: fn(&Path)) -> Case {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n\
         \x20   let read = fs.read_only().restrict_to(\"root\")\n\
         \x20   let write = fs.write_only().restrict_to(\"root\")\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n{SHOWN}"
    );
    Case::file(&format!("file_ops_{name}"), &src)
        .with_file("root/inside.txt", "inside text")
        .with_file("root/sub/nested.txt", "nested text")
        .with_file("outside/secret.txt", "outside text")
        .with_setup(setup)
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
fn link(dir: &Path, at: &str, target: impl AsRef<Path>) {
    let target = target.as_ref();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, dir.join(at));
    #[cfg(windows)]
    let made = {
        let at = dir.join(at);
        let target = std::path::PathBuf::from(target.to_string_lossy().replace('/', "\\"));
        let resolved = at.parent().expect("a parent").join(&target);
        match resolved.is_dir() {
            true => std::os::windows::fs::symlink_dir(&target, &at),
            false => std::os::windows::fs::symlink_file(&target, &at),
        }
    };
    made.expect("create a symlink");
}

fn nothing(_: &Path) {}

const SYMLINK: &str = "is a symlink, and confined access never follows one";
const HARD_LINK: &str = "has more than one hard link, and confined access never opens one";

#[test]
fn the_operations_work_on_ordinary_files() {
    let case = case(
        "works",
        r#"    io.println(flag(read.exists(Path.new("inside.txt"))))?
    io.println(flag(read.exists(Path.new("sub"))))?
    io.println(flag(read.exists(Path.new("."))))?
    io.println(flag(read.exists(Path.new("nothing.txt"))))?
    io.println(flag(read.exists(Path.new("nothing/at/all.txt"))))?
    io.println(described(read.metadata(Path.new("inside.txt"))))?
    io.println(described(read.metadata(Path.new("sub"))))?
    io.println(described(read.metadata(Path.new("nothing.txt"))))?
    io.println(done(write.append(Path.new("log.txt"), "one\n")))?
    io.println(done(write.append(Path.new("log.txt"), "two\n")))?
    io.println(described(read.metadata(Path.new("log.txt"))))?
    io.println(done(write.append(Path.new("sub"), "x")))?
    io.println(done(write.rename(Path.new("log.txt"), Path.new("sub/log.txt"))))?
    io.println(done(write.rename(Path.new("inside.txt"), Path.new("sub/log.txt"))))?
    io.println(done(write.rename(Path.new("sub"), Path.new("moved"))))?
    io.println(done(write.rename(Path.new("sub/log.txt"), Path.new("sub"))))?
    io.println(done(write.rename(Path.new("nothing.txt"), Path.new("x.txt"))))?
    io.println(done(write.remove_file(Path.new("sub"))))?
    io.println(done(write.remove_dir(Path.new("sub"))))?
    io.println(done(write.remove_dir(Path.new("sub/log.txt"))))?
    io.println(done(write.remove_file(Path.new("sub/log.txt"))))?
    io.println(done(write.remove_file(Path.new("sub/log.txt"))))?
    io.println(done(write.remove_file(Path.new("sub/nested.txt"))))?
    io.println(done(write.remove_dir(Path.new("sub"))))?
    io.println(flag(read.exists(Path.new("sub"))))?
    io.println(done(write.remove_dir(Path.new("sub"))))?
    let one = fs.write_only().restrict_to_file("root/one.txt")
    io.println(done(one.append("a")))?
    io.println(done(one.append("b")))?
    io.println(described(fs.read_only().restrict_to_file("root/one.txt").metadata()))?
    io.println(described(fs.read_only().restrict_to_file("root/none.txt").metadata()))?"#,
        nothing,
    );
    assert_eq!(
        printed(&case),
        "true\ntrue\ntrue\nfalse\nfalse\n\
         file 11\ndirectory 0\nREFUSED: `nothing.txt`: not found\n\
         done\ndone\nfile 8\n\
         REFUSED: `sub` is not a regular file\n\
         done\ndone\n\
         REFUSED: `sub` is not a regular file\n\
         REFUSED: `sub` is not a regular file\n\
         REFUSED: `nothing.txt`: not found\n\
         REFUSED: `sub` is not a regular file\n\
         REFUSED: `sub`: directory not empty\n\
         REFUSED: `sub/log.txt` is a symlink or not a directory, \
         and confined access never follows a symlink\n\
         done\n\
         REFUSED: `sub/log.txt`: not found\n\
         done\ndone\nfalse\n\
         REFUSED: `sub`: not found\n\
         done\ndone\nfile 2\nREFUSED: `none.txt`: not found\n"
    );
}

#[test]
fn dot_dot_and_absolute_paths_are_refused_by_every_operation() {
    let case = case(
        "escapes",
        r#"    io.println(flag(read.exists(Path.new("../outside/secret.txt"))))?
    io.println(described(read.metadata(Path.new("../outside/secret.txt"))))?
    io.println(done(write.append(Path.new("../outside/secret.txt"), "x")))?
    io.println(done(write.remove_file(Path.new("../outside/secret.txt"))))?
    io.println(done(write.remove_dir(Path.new("../outside"))))?
    io.println(done(write.rename(Path.new("../outside/secret.txt"), Path.new("s.txt"))))?
    io.println(done(write.rename(Path.new("inside.txt"), Path.new("../outside/secret.txt"))))?
    io.println(flag(read.exists(Path.new("/etc/passwd"))))?
    io.println(done(write.remove_file(Path.new("/etc/passwd"))))?
    io.println(done(write.rename(Path.new("inside.txt"), Path.new("/tmp/x"))))?"#,
        nothing,
    );
    let leaves = |p: &str| format!("REFUSED: `{p}` leads outside the capability's root\n");
    let absolute = |p: &str| {
        format!(
            "REFUSED: `{p}` is absolute; a directory capability takes paths relative to its root\n"
        )
    };
    assert_eq!(
        printed(&case),
        format!(
            "{}{}{}{}{}{}{}{}{}{}",
            leaves("../outside/secret.txt"),
            leaves("../outside/secret.txt"),
            leaves("../outside/secret.txt"),
            leaves("../outside/secret.txt"),
            leaves("../outside"),
            leaves("../outside/secret.txt"),
            leaves("../outside/secret.txt"),
            absolute("/etc/passwd"),
            absolute("/etc/passwd"),
            absolute("/tmp/x"),
        )
    );
}

#[test]
fn a_symlink_is_refused_as_the_target_of_every_operation() {
    let case = case(
        "symlinks",
        r#"    io.println(done(write.remove_file(Path.new("link.txt"))))?
    io.println(done(write.rename(Path.new("link.txt"), Path.new("moved.txt"))))?
    io.println(done(write.rename(Path.new("inside.txt"), Path.new("link.txt"))))?
    io.println(done(write.append(Path.new("link.txt"), "x")))?
    io.println(described(read.metadata(Path.new("link.txt"))))?
    io.println(flag(read.exists(Path.new("link.txt"))))?
    io.println(flag(read.exists(Path.new("dangling.txt"))))?
    io.println(done(write.remove_file(Path.new("out/secret.txt"))))?
    io.println(done(write.remove_dir(Path.new("out"))))?
    io.println(done(write.rename(Path.new("out/secret.txt"), Path.new("s.txt"))))?"#,
        |dir| {
            link(dir, "root/link.txt", "../outside/secret.txt");
            link(dir, "root/dangling.txt", "../nothing-here");
            link(dir, "root/out", "../outside");
        },
    );
    let file = format!("REFUSED: `link.txt` {SYMLINK}\n");
    let dangling = format!("REFUSED: `dangling.txt` {SYMLINK}\n");
    let dir = "REFUSED: `out` is a symlink or not a directory, \
               and confined access never follows a symlink\n";
    let seen = printed(&case);
    // Windows can tell a symlink where a directory is needed from anything
    // else that is not a directory, and says so.
    let seen = seen.replace(
        "is a symlink or not a directory, and confined access never follows a symlink",
        "is a symlink, and confined access never follows one",
    );
    let dir = dir.replace(
        "is a symlink or not a directory, and confined access never follows a symlink",
        "is a symlink, and confined access never follows one",
    );
    assert_eq!(
        seen,
        format!("{file}{file}{file}{file}{file}{file}{dangling}{dir}{dir}{dir}")
    );
}

#[test]
fn a_hard_link_is_refused_as_the_target_of_every_operation_that_opens_it() {
    let case = case(
        "hard_links",
        r#"    io.println(done(write.remove_file(Path.new("leak.txt"))))?
    io.println(done(write.rename(Path.new("leak.txt"), Path.new("moved.txt"))))?
    io.println(done(write.rename(Path.new("inside.txt"), Path.new("leak.txt"))))?
    io.println(done(write.append(Path.new("leak.txt"), "x")))?
    io.println(described(read.metadata(Path.new("leak.txt"))))?
    io.println(flag(read.exists(Path.new("leak.txt"))))?"#,
        |dir| {
            std::fs::hard_link(dir.join("outside/secret.txt"), dir.join("root/leak.txt"))
                .expect("create a hard link");
        },
    );
    let refused = format!("REFUSED: `leak.txt` {HARD_LINK}\n");
    assert_eq!(
        printed(&case),
        format!("{refused}{refused}{refused}{refused}{refused}true\n")
    );
}
