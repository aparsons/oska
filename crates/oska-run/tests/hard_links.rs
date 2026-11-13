//! Hard links beneath a directory or file capability (review fix F4 of
//! #93). A hard link is the same file under a second name, so one
//! planted beneath a root, such as in an upload directory, would expose
//! or overwrite a file outside it. Opening a regular file beneath a
//! capability therefore refuses one with more than one link, for reads
//! and writes, after opening it and before reading or emptying it. A
//! write that creates a new file is unaffected.
//!
//! The tests run Oska programs on [`oska_run::RealHost`] against a real
//! temporary directory with real hard links, on Unix and Windows.

#![cfg(any(unix, windows))]

mod common;

use common::TempTree;

/// A [`TempTree`] with `root/inside.txt` ("inside text"), and
/// `root/leak.txt`, a hard link to `outside/secret.txt` ("outside text").
fn linked_tree(test: &str) -> TempTree {
    let tree = TempTree::new(&format!("hardlink-{test}"));
    tree.write("root/inside.txt", "inside text");
    tree.write("outside/secret.txt", "outside text");
    std::fs::hard_link(tree.path("outside/secret.txt"), tree.path("root/leak.txt"))
        .expect("create a hard link");
    tree
}

const REFUSAL: &str = "REFUSED: `leak.txt` has more than one hard link, \
                       and confined access never opens one";

#[test]
fn a_directory_capability_refuses_to_read_a_hard_linked_file() {
    let tree = linked_tree("dir-read");
    let seen = tree.saw("    say(shown(read.read_to_string(Path.new(\"leak.txt\"))))");
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_directory_capability_refuses_to_write_a_hard_linked_file() {
    let tree = linked_tree("dir-write");
    let seen = tree.saw("    say(done(write.write(Path.new(\"leak.txt\"), \"overwritten\")))");
    assert_eq!(seen, REFUSAL);
    assert_eq!(tree.file("outside/secret.txt"), "outside text");
}

#[test]
fn a_file_capability_refuses_to_read_a_hard_linked_file() {
    let tree = linked_tree("file-read");
    let seen = tree.saw(
        "    let leak = fs.read_only().restrict_to_file(\"{root}/leak.txt\")\n    \
         say(shown(leak.read_to_string()))",
    );
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_file_capability_refuses_to_write_a_hard_linked_file() {
    let tree = linked_tree("file-write");
    let seen = tree.saw(
        "    let leak = fs.write_only().restrict_to_file(\"{root}/leak.txt\")\n    \
         say(done(leak.write(\"overwritten\")))",
    );
    assert_eq!(seen, REFUSAL);
    assert_eq!(tree.file("outside/secret.txt"), "outside text");
}

#[test]
fn a_file_from_a_directory_capability_refuses_a_hard_link() {
    let tree = linked_tree("dir-file");
    let seen =
        tree.saw("    say(shown(read.restrict_to_file(Path.new(\"leak.txt\")).read_to_string()))");
    assert_eq!(seen, REFUSAL);
}

#[test]
fn a_file_with_one_link_is_read_and_rewritten() {
    let tree = linked_tree("one-link");
    let seen = tree.saw(
        "    say(shown(read.read_to_string(Path.new(\"inside.txt\"))))\n    \
         say(done(write.write(Path.new(\"inside.txt\"), \"rewritten\")))",
    );
    assert_eq!(seen, "inside text\ndone");
    assert_eq!(tree.file("root/inside.txt"), "rewritten");
}

#[test]
fn a_write_that_creates_a_file_is_unaffected() {
    let tree = linked_tree("create");
    let seen = tree.saw(
        "    say(done(write.write(Path.new(\"new.txt\"), \"new text\")))\n    \
         say(shown(read.read_to_string(Path.new(\"new.txt\"))))",
    );
    assert_eq!(seen, "done\nnew text");
}
