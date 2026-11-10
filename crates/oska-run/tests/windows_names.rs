//! Windows names beneath a directory or file capability (review fix F5 of
//! #93). On Windows, a reserved device name such as `CON` or `nul.txt`
//! opens a device in every directory, and a `:` in a name opens an
//! alternate data stream of a file, so neither is a file beneath the
//! root. Each is refused in any position of a path a capability walks,
//! for reads, writes, `create_dir`, `list_dir` and narrowing.
//!
//! The tests run Oska programs on [`oska_run::RealHost`] against a real
//! temporary directory (Windows only: the names are ordinary elsewhere).
//! The Windows CI job runs them.

#![cfg(windows)]

mod common;

use common::TempTree;

/// What reading `name` beneath the root shows.
fn read(tree: &TempTree, name: &str) -> String {
    tree.saw(&format!(
        "    say(shown(read.read_to_string(Path.new(\"{name}\"))))"
    ))
}

/// What writing `name` beneath the root shows.
fn write(tree: &TempTree, name: &str) -> String {
    tree.saw(&format!(
        "    say(done(write.write(Path.new(\"{name}\"), \"text\")))"
    ))
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
fn a_device_name_is_refused_for_a_read() {
    let tree = TempTree::new("winname-read");
    for name in ["CON", "nul", "Aux", "PRN", "COM1", "lpt9"] {
        assert_eq!(read(&tree, name), device(name));
    }
}

#[test]
fn a_device_name_with_an_extension_is_refused_for_a_write() {
    let tree = TempTree::new("winname-write");
    for name in ["nul.txt", "CON.log", "com3.tar.gz", "LPT1 .txt"] {
        assert_eq!(write(&tree, name), device(name));
    }
}

#[test]
fn a_device_name_as_a_directory_is_refused() {
    let tree = TempTree::new("winname-dir");
    assert_eq!(
        tree.saw("    say(done(write.create_dir(Path.new(\"aux\"))))"),
        device("aux")
    );
    assert_eq!(
        tree.saw("    say(listed(read.list_dir(Path.new(\"nul\"))))"),
        device("nul")
    );
    assert_eq!(read(&tree, "con/inside.txt"), device("con"));
}

#[test]
fn a_device_name_is_refused_for_a_file_capability() {
    let tree = TempTree::new("winname-file");
    let seen = tree.saw(
        "    let console = write.restrict_to_file(Path.new(\"CON\"))\n    \
         say(done(console.write(\"text\")))",
    );
    // Attenuation never fails: the capability reaches nothing, and says why.
    assert_eq!(
        seen,
        format!("REFUSED: cannot restrict to `CON`: {}", &device("CON")[9..])
    );
}

#[test]
fn a_narrowing_to_a_device_name_reaches_nothing() {
    let tree = TempTree::new("winname-narrow");
    let seen = tree.saw(
        "    let printer = read.restrict_to(Path.new(\"prn\"))\n    \
         say(listed(printer.list_dir(Path.new(\".\"))))",
    );
    assert!(seen.contains(&device("prn")[9..]), "{seen}");
}

#[test]
fn an_alternate_data_stream_is_refused() {
    let tree = TempTree::new("winname-stream");
    tree.write("root/inside.txt", "inside text");
    assert_eq!(
        read(&tree, "inside.txt:hidden"),
        stream("inside.txt:hidden")
    );
    assert_eq!(
        write(&tree, "inside.txt:hidden"),
        stream("inside.txt:hidden")
    );
    assert_eq!(
        read(&tree, "inside.txt::$DATA"),
        stream("inside.txt::$DATA")
    );
}

#[test]
fn names_that_only_start_like_a_device_are_files() {
    let tree = TempTree::new("winname-ok");
    let seen = tree.saw(
        "    say(done(write.write(Path.new(\"console.txt\"), \"a\")))\n    \
         say(done(write.write(Path.new(\"com10\"), \"b\")))\n    \
         say(done(write.write(Path.new(\"nul_file\"), \"c\")))\n    \
         say(shown(read.read_to_string(Path.new(\"console.txt\"))))",
    );
    assert_eq!(seen, "done\ndone\ndone\na");
}

#[test]
fn reads_and_writes_beneath_the_root_work() {
    let tree = TempTree::new("winname-plain");
    let seen = tree.saw(
        "    say(done(write.create_dir(Path.new(\"sub\"))))\n    \
         say(done(write.write(Path.new(\"sub/a.txt\"), \"a text\")))\n    \
         say(shown(read.read_to_string(Path.new(\"sub/a.txt\"))))\n    \
         say(listed(read.list_dir(Path.new(\"sub\"))))\n    \
         let file = write.restrict_to_file(Path.new(\"sub/b.txt\"))\n    \
         say(done(file.write(\"b text\")))\n    \
         let again = fs.read_only().restrict_to_file(\"{root}/sub/b.txt\")\n    \
         say(shown(again.read_to_string()))",
    );
    assert_eq!(seen, "done\ndone\na text\na.txt\ndone\nb text");
}
