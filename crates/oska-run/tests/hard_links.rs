//! Hard links beneath a directory or file capability (review fix F4 of
//! #93). A hard link is the same file under a second name, so one
//! planted beneath a root, such as in an upload directory, would expose
//! or overwrite a file outside it. Opening a regular file beneath a
//! capability therefore refuses one with more than one link, for reads
//! and writes, after opening it and before reading or emptying it. A
//! write that creates a new file is unaffected.
//!
//! The tests run Oska programs on [`RealHost`] against a real temporary
//! directory with real hard links (Unix only; Windows keeps its
//! path-based checks).

#![cfg(unix)]

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::SHOWN;
use oska_run::{run, RealHost};

/// A fresh directory under the system temp dir, removed on drop:
///
/// ```text
/// root/inside.txt         "inside text"
/// root/leak.txt           a hard link to outside/secret.txt
/// outside/secret.txt      "outside text"
/// report/                 where a program writes what it saw
/// ```
struct LinkedTree {
    base: PathBuf,
}

impl LinkedTree {
    fn new(test: &str) -> LinkedTree {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-hardlink-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        for dir in ["root", "outside", "report"] {
            std::fs::create_dir_all(base.join(dir)).expect("create a directory");
        }
        std::fs::write(base.join("root/inside.txt"), "inside text").expect("write inside");
        std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write outside");
        std::fs::hard_link(base.join("outside/secret.txt"), base.join("root/leak.txt"))
            .expect("create a hard link");
        let base = std::fs::canonicalize(&base).expect("canonicalize the tree");
        LinkedTree { base }
    }

    fn path(&self, at: &str) -> PathBuf {
        self.base.join(at)
    }

    /// The text of the file at `at`.
    fn file(&self, at: &str) -> String {
        std::fs::read_to_string(self.path(at)).expect("read a file")
    }

    /// What `body` saw, each `say(...)` one line. `main` is given
    /// `read: DirRead` and `write: DirWrite` on `root`, and `fs: Fs` to
    /// make file capabilities, with `{root}` in `body` replaced by the
    /// root's absolute path.
    fn saw(&self, body: &str) -> String {
        let root = self.path("root").display().to_string();
        let src = format!(
            "fn main(fs: Fs) -> Result<(), IoError> {{\n\
             \x20   let read = fs.read_only().restrict_to(\"{root}\")\n\
             \x20   let write = fs.write_only().restrict_to(\"{root}\")\n\
             \x20   var seen: Untrusted<String> = \"\"\n\
             {body}\n\
             \x20   fs.write_only().restrict_to(\"{report}\").write(Path.new(\"seen.txt\"), seen)\n\
             }}\n\
             \n\
             fn said(seen: Untrusted<String>, line: Untrusted<String>) -> Untrusted<String> {{\n\
             \x20   \"{{seen}}\\n{{line}}\"\n\
             }}\n{SHOWN}",
            report = self.path("report").display(),
            body = body
                .replace("say(", "seen = said(seen, ")
                .replace("{root}", &root),
        );
        let outcome = run(&src, &mut RealHost::new(), &["prog.osk".to_string()]);
        assert!(
            outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
            "{src} should run: {outcome:#?}"
        );
        self.file("report/seen.txt")
            .trim_start_matches('\n')
            .to_string()
    }
}

impl Drop for LinkedTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

const REFUSAL: &str = "REFUSED: `leak.txt` has more than one hard link, \
                       and confined access never opens one";

#[test]
fn a_directory_capability_refuses_to_read_a_hard_linked_file() {
    let tree = LinkedTree::new("dir-read");
    let seen = tree.saw("    say(shown(read.read_to_string(Path.new(\"leak.txt\"))))");
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_directory_capability_refuses_to_write_a_hard_linked_file() {
    let tree = LinkedTree::new("dir-write");
    let seen = tree.saw("    say(done(write.write(Path.new(\"leak.txt\"), \"overwritten\")))");
    assert_eq!(seen, REFUSAL);
    assert_eq!(tree.file("outside/secret.txt"), "outside text");
}

#[test]
fn a_file_capability_refuses_to_read_a_hard_linked_file() {
    let tree = LinkedTree::new("file-read");
    let seen = tree.saw(
        "    let leak = fs.read_only().restrict_to_file(\"{root}/leak.txt\")\n    \
         say(shown(leak.read_to_string()))",
    );
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_file_capability_refuses_to_write_a_hard_linked_file() {
    let tree = LinkedTree::new("file-write");
    let seen = tree.saw(
        "    let leak = fs.write_only().restrict_to_file(\"{root}/leak.txt\")\n    \
         say(done(leak.write(\"overwritten\")))",
    );
    assert_eq!(seen, REFUSAL);
    assert_eq!(tree.file("outside/secret.txt"), "outside text");
}

#[test]
fn a_file_from_a_directory_capability_refuses_a_hard_link() {
    let tree = LinkedTree::new("dir-file");
    let seen =
        tree.saw("    say(shown(read.restrict_to_file(Path.new(\"leak.txt\")).read_to_string()))");
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_file_with_one_link_is_read_and_rewritten() {
    let tree = LinkedTree::new("one-link");
    let seen = tree.saw(
        "    say(shown(read.read_to_string(Path.new(\"inside.txt\"))))\n    \
         say(done(write.write(Path.new(\"inside.txt\"), \"rewritten\")))",
    );
    assert_eq!(seen, "inside text\ndone");
    assert_eq!(tree.file("root/inside.txt"), "rewritten");
}

#[test]
fn a_write_that_creates_a_file_is_unaffected() {
    let tree = LinkedTree::new("create");
    let seen = tree.saw(
        "    say(done(write.write(Path.new(\"new.txt\"), \"new text\")))\n    \
         say(shown(read.read_to_string(Path.new(\"new.txt\"))))",
    );
    assert_eq!(seen, "done\nnew text");
}
