//! Race-free confinement on every OS (#175): a directory or file
//! capability walks its paths one component at a time beneath handles it
//! holds, never following a symlink or, on Windows, any other reparse
//! point (a junction or a mount point), and never opening a file with more
//! than one link (`hard_links.rs`).
//!
//! The tests run Oska programs on [`RealHost`] against a real temporary
//! directory:
//!
//! - **A component swapped during a walk**, on Unix and Windows: a program
//!   narrows to `sub`, which opens it, and then the test moves `sub`
//!   away and puts a link to a directory outside the root in its place
//!   (a symlink on Unix, a junction on Windows), at a moment the program
//!   chooses. What the program opened stays the directory it opened, and
//!   a new walk through `sub` is refused.
//! - **Reparse points**, on Windows only: junctions, symlinks to
//!   directories and to files, and a volume mount point, at each position
//!   of a path, refused with the words Linux and macOS use for a symlink.
//!   The Windows CI jobs run them.

#![cfg(any(unix, windows))]

mod common;

use std::path::Path;

use common::{literal, TempTree, SHOWN};
use oska_run::{run, Host, Output, RealHost, RootHandle};

/// A [`TempTree`] holding:
///
/// ```text
/// root/inside.txt         "inside text"
/// root/sub/nested.txt     "nested text"
/// outside/secret.txt      "outside text"
/// ```
fn tree(test: &str) -> TempTree {
    let tree = TempTree::new(&format!("walks-{test}"));
    tree.write("root/inside.txt", "inside text");
    tree.write("root/sub/nested.txt", "nested text");
    tree.write("outside/secret.txt", "outside text");
    tree
}

/// A link at `at` to the directory `target`, both beneath `tree`, that a
/// walk must not follow: a symlink on Unix, a junction on Windows.
fn link_dir(tree: &TempTree, at: &str, target: &str) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(tree.path(target), tree.path(at)).expect("create a symlink");
    #[cfg(windows)]
    windows::junction(&tree.path(at), &tree.path(target));
}

/// The names in the directory `at` beneath `tree`, sorted.
fn names_in(tree: &TempTree, at: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(tree.path(at))
        .expect("list a directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .collect();
    names.sort();
    names
}

/// [`RealHost`] with its stdout recorded, which moves `root/sub` to
/// `root/moved` and links `root/sub` to `outside` when the program prints
/// `swap`. Every other operation goes straight to the real OS.
struct Swapping<'t> {
    real: RealHost,
    tree: &'t TempTree,
    stdout: String,
}

impl Host for Swapping<'_> {
    fn write_stdout(&mut self, text: &str) -> std::io::Result<()> {
        if text == "swap\n" {
            std::fs::rename(self.tree.path("root/sub"), self.tree.path("root/moved"))
                .expect("move `sub` away");
            link_dir(self.tree, "root/sub", "outside");
        }
        self.stdout.push_str(text);
        Ok(())
    }
    fn write_stderr(&mut self, text: &str) -> std::io::Result<()> {
        self.real.write_stderr(text)
    }
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        self.real.read_line()
    }
    fn env_var(&mut self, name: &str) -> Option<String> {
        self.real.env_var(name)
    }
    fn now_millis(&mut self) -> i128 {
        self.real.now_millis()
    }
    fn next_random(&mut self) -> u64 {
        self.real.next_random()
    }
    fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.real.open_root(path)
    }
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<RootHandle> {
        self.real.open_beneath(root, names)
    }
    fn open_file_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.real.open_file_root(path)
    }
    fn open_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<RootHandle> {
        self.real.open_file_beneath(root, names)
    }
    fn read_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<String> {
        self.real.read_file_beneath(root, names)
    }
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &str,
    ) -> std::io::Result<()> {
        self.real.write_file_beneath(root, names, contents)
    }
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<()> {
        self.real.create_dir_beneath(root, names)
    }
    fn list_dir_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<Vec<String>> {
        self.real.list_dir_beneath(root, names)
    }
    fn printed(&self) -> Vec<String> {
        Vec::new()
    }
    fn take_output(&mut self) -> Output {
        Output {
            stdout: std::mem::take(&mut self.stdout),
            stderr: String::new(),
        }
    }
}

/// The lines `body` prints on a [`Swapping`] host, each `say(...)` one,
/// with `read: DirRead` and `write: DirWrite` on `root` in scope and the
/// [`SHOWN`] helpers.
fn said_while_swapping(tree: &TempTree, body: &str) -> Vec<String> {
    let root = literal(tree.path("root"));
    let src = format!(
        "fn main(io: Stdout, fs: Fs) {{\n\
         \x20   let read = fs.read_only().restrict_to(\"{root}\")\n\
         \x20   let write = fs.write_only().restrict_to(\"{root}\")\n\
         {}\n\
         }}\n{SHOWN}",
        body.replace("say(", "let _ = io.println(")
    );
    let mut host = Swapping {
        real: RealHost::new(),
        tree,
        stdout: String::new(),
    };
    let outcome = run(&src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout.lines().map(str::to_string).collect()
}

// ---- A component swapped during a walk ----------------------------------------------

#[test]
fn what_a_walk_opened_stays_put_when_a_component_is_swapped() {
    let tree = tree("swap-held");
    let said = said_while_swapping(
        &tree,
        "    let held = read.restrict_to(Path.new(\"sub\"))\n    \
         let held_write = write.restrict_to(Path.new(\"sub\"))\n    \
         let nested = read.restrict_to_file(Path.new(\"sub/nested.txt\"))\n    \
         say(\"swap\")\n    \
         say(shown(held.read_to_string(Path.new(\"nested.txt\"))))\n    \
         say(shown(nested.read_to_string()))\n    \
         say(listed(held.list_dir(Path.new(\".\"))))\n    \
         say(shown(held.read_to_string(Path.new(\"secret.txt\"))))\n    \
         say(done(held_write.write(Path.new(\"new.txt\"), \"new text\")))",
    );
    assert_eq!(
        said[..4],
        ["swap", "nested text", "nested text", "nested.txt"]
    );
    // The directory outside is not reached: it has no `nested.txt`, and
    // its `secret.txt` is not found where the walk looks.
    assert!(
        said[4].starts_with("REFUSED: ") && !said[4].contains("outside text"),
        "{said:?}"
    );
    assert_eq!(said[5], "done");
    assert_eq!(tree.file("root/moved/new.txt"), "new text");
    assert_eq!(names_in(&tree, "outside"), ["secret.txt"]);
}

#[test]
fn a_walk_through_a_swapped_component_is_refused() {
    let tree = tree("swap-new");
    let said = said_while_swapping(
        &tree,
        "    say(\"swap\")\n    \
         say(shown(read.read_to_string(Path.new(\"sub/secret.txt\"))))\n    \
         say(listed(read.list_dir(Path.new(\"sub\"))))\n    \
         say(done(write.write(Path.new(\"sub/planted.txt\"), \"pwned\")))\n    \
         say(done(write.create_dir(Path.new(\"sub/planted\"))))\n    \
         say(listed(read.restrict_to(Path.new(\"sub\")).list_dir(Path.new(\".\"))))",
    );
    assert_eq!(said[0], "swap");
    for line in &said[1..] {
        assert!(
            line.starts_with("REFUSED: ") && line.contains("`sub` is a symlink"),
            "{said:?}"
        );
    }
    assert_eq!(said.len(), 6, "{said:?}");
    assert_eq!(names_in(&tree, "outside"), ["secret.txt"]);
}

// ---- Reparse points on Windows ------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use std::path::Path;
    use std::process::Command;

    use super::common::{literal, TempTree};
    use super::{names_in, tree};

    /// The words a walk refuses a reparse point at `path` with, as Linux
    /// and macOS refuse a symlink.
    fn refusal(path: &str) -> String {
        format!("REFUSED: `{path}` is a symlink, and confined access never follows one")
    }

    /// `path` without the `\\?\` that `canonicalize` puts before it, which
    /// `mklink` and `mountvol` do not take.
    fn plain(path: &Path) -> String {
        let text = path.display().to_string();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    }

    /// Run `command`, which must succeed.
    #[track_caller]
    fn ran(command: &mut Command) -> String {
        let out = command.output().expect("run a command");
        assert!(out.status.success(), "{command:?} failed: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A junction at `at` to the directory `target`.
    pub fn junction(at: &Path, target: &Path) {
        ran(Command::new("cmd").args(["/C", "mklink", "/J", &plain(at), &plain(target)]));
    }

    /// A symlink at `at` beneath `tree` to `target` beneath it: a
    /// directory symlink when `target` is a directory.
    fn symlink(tree: &TempTree, at: &str, target: &str) {
        let (at, target) = (tree.path(at), tree.path(target));
        let made = match target.is_dir() {
            true => std::os::windows::fs::symlink_dir(&target, &at),
            false => std::os::windows::fs::symlink_file(&target, &at),
        };
        made.expect("create a symlink");
    }

    /// A volume mount point: the volume holding the temp dir mounted at
    /// an empty directory, unmounted on drop before the tree is removed.
    struct MountPoint {
        at: String,
    }

    impl MountPoint {
        fn new(tree: &TempTree, at: &str) -> MountPoint {
            std::fs::create_dir(tree.path(at)).expect("create the mount directory");
            let at = format!("{}\\", plain(&tree.path(at)));
            let drive = format!("{}\\", &at[..2]);
            let volume = ran(Command::new("mountvol").args([&drive, "/L"]));
            ran(Command::new("mountvol").args([&at, volume.trim()]));
            MountPoint { at }
        }
    }

    impl Drop for MountPoint {
        fn drop(&mut self) {
            let _ = Command::new("mountvol").args([&self.at, "/D"]).output();
        }
    }

    #[test]
    fn a_junction_at_any_component_is_refused() {
        let tree = tree("junction");
        junction(&tree.path("root/out"), &tree.path("outside"));
        junction(&tree.path("root/sub/out"), &tree.path("outside"));
        let seen = tree.saw(
            "    say(shown(read.read_to_string(Path.new(\"out/secret.txt\"))))\n    \
             say(shown(read.read_to_string(Path.new(\"sub/out/secret.txt\"))))\n    \
             say(listed(read.list_dir(Path.new(\"out\"))))\n    \
             say(done(write.write(Path.new(\"out/planted.txt\"), \"pwned\")))\n    \
             say(done(write.create_dir(Path.new(\"sub/out/planted\"))))\n    \
             say(done(write.create_dir(Path.new(\"out\"))))\n    \
             say(shown(read.restrict_to(Path.new(\"out\")).read_to_string(Path.new(\"secret.txt\"))))",
        );
        let expected = [
            refusal("out"),
            refusal("sub/out"),
            refusal("out"),
            refusal("out"),
            refusal("sub/out"),
            refusal("out"),
        ];
        let lines: Vec<&str> = seen.lines().collect();
        assert_eq!(lines[..6], expected, "{seen}");
        assert!(lines[6].contains("`out` is a symlink"), "{seen}");
        assert_eq!(names_in(&tree, "outside"), ["secret.txt"]);
    }

    #[test]
    fn a_junction_pointing_inside_the_root_is_refused() {
        let tree = tree("junction-inside");
        junction(&tree.path("root/alias"), &tree.path("root/sub"));
        let seen = tree.saw(
            "    say(shown(read.read_to_string(Path.new(\"alias/nested.txt\"))))\n    \
             say(listed(read.list_dir(Path.new(\"alias\"))))",
        );
        assert_eq!(seen, format!("{}\n{}", refusal("alias"), refusal("alias")));
    }

    #[test]
    fn a_junction_as_the_root_is_refused() {
        let tree = tree("junction-root");
        junction(&tree.path("root-link"), &tree.path("root"));
        let link = literal(tree.path("root-link"));
        let seen = tree.saw(&format!(
            "    let linked = fs.read_only().restrict_to(\"{link}\")\n    \
             say(shown(linked.read_to_string(Path.new(\"inside.txt\"))))"
        ));
        assert!(
            seen.starts_with("REFUSED: ") && seen.contains("`root-link` is a symlink"),
            "{seen}"
        );
    }

    #[test]
    fn a_symlink_to_a_directory_or_a_file_is_refused() {
        let tree = tree("symlink");
        symlink(&tree, "root/sub/out", "outside");
        symlink(&tree, "root/escape.txt", "outside/secret.txt");
        symlink(&tree, "root/alias.txt", "root/inside.txt");
        let seen = tree.saw(
            "    say(shown(read.read_to_string(Path.new(\"sub/out/secret.txt\"))))\n    \
             say(shown(read.read_to_string(Path.new(\"escape.txt\"))))\n    \
             say(done(write.write(Path.new(\"escape.txt\"), \"pwned\")))\n    \
             say(shown(read.read_to_string(Path.new(\"alias.txt\"))))\n    \
             say(done(write.write(Path.new(\"alias.txt\"), \"pwned\")))",
        );
        let expected = [
            refusal("sub/out"),
            refusal("escape.txt"),
            refusal("escape.txt"),
            refusal("alias.txt"),
            refusal("alias.txt"),
        ];
        assert_eq!(seen, expected.join("\n"));
        assert_eq!(tree.file("outside/secret.txt"), "outside text");
        assert_eq!(tree.file("root/inside.txt"), "inside text");
    }

    #[test]
    fn a_dangling_symlink_is_refused_and_creates_nothing() {
        let tree = tree("dangling");
        std::os::windows::fs::symlink_file(
            tree.path("outside/planted.txt"),
            tree.path("root/plant.txt"),
        )
        .expect("create a symlink");
        let seen = tree.saw(
            "    say(done(write.write(Path.new(\"plant.txt\"), \"pwned\")))\n    \
             say(shown(read.read_to_string(Path.new(\"plant.txt\"))))",
        );
        assert_eq!(
            seen,
            format!("{}\n{}", refusal("plant.txt"), refusal("plant.txt"))
        );
        assert_eq!(names_in(&tree, "outside"), ["secret.txt"]);
    }

    #[test]
    fn a_mount_point_is_refused() {
        let tree = tree("mount");
        let _mounted = MountPoint::new(&tree, "root/mnt");
        let seen = tree.saw(
            "    say(listed(read.list_dir(Path.new(\"mnt\"))))\n    \
             say(shown(read.read_to_string(Path.new(\"mnt/anything.txt\"))))\n    \
             say(done(write.write(Path.new(\"mnt/planted.txt\"), \"pwned\")))",
        );
        let expected = [refusal("mnt"), refusal("mnt"), refusal("mnt")];
        assert_eq!(seen, expected.join("\n"));
    }

    #[test]
    fn a_plain_directory_and_file_are_read_written_and_listed() {
        let tree = tree("plain");
        let seen = tree.saw(
            "    say(shown(read.read_to_string(Path.new(\"sub/nested.txt\"))))\n    \
             say(done(write.write(Path.new(\"sub/new.txt\"), \"new text\")))\n    \
             say(done(write.write(Path.new(\"inside.txt\"), \"short\")))\n    \
             say(done(write.create_dir(Path.new(\"sub/deeper\"))))\n    \
             say(done(write.create_dir(Path.new(\"sub/deeper\"))))\n    \
             say(listed(read.list_dir(Path.new(\"sub\"))))\n    \
             say(listed(read.list_dir(Path.new(\".\"))))",
        );
        assert_eq!(
            seen,
            "nested text\ndone\ndone\ndone\ndone\ndeeper,nested.txt,new.txt\ninside.txt,sub"
        );
        assert_eq!(tree.file("root/inside.txt"), "short");
        assert_eq!(tree.file("root/sub/new.txt"), "new text");
    }
}
