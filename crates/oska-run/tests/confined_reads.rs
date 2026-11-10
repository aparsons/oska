//! Race-free confined reads (#94): a read through a restricted filesystem
//! walks its path one component at a time beneath a handle for the root,
//! never following a symlink, so another process that swaps a directory
//! for a symlink mid-run cannot steer it outside the root.
//!
//! Every test runs an Oska program on [`RealHost`] against a real
//! temporary directory with real symlinks. A symlink planted at a
//! component stands for one swapped in there at any moment of the walk:
//! the walk refuses it either way, so the race is tested without timing.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use oska_run::{run, Host, Output, RealHost, RootHandle};

/// A fresh directory under the system temp dir, removed on drop:
///
/// ```text
/// root/inside.txt         "inside text"
/// root/sub/nested.txt     "nested text"
/// outside/secret.txt      "outside text"
/// ```
struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-confined-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("root/sub")).expect("create the root");
        std::fs::create_dir_all(base.join("outside")).expect("create the outside");
        std::fs::write(base.join("root/inside.txt"), "inside text").expect("write inside");
        std::fs::write(base.join("root/sub/nested.txt"), "nested text").expect("write nested");
        std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write outside");
        let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
        Sandbox { base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn outside(&self) -> PathBuf {
        self.base.join("outside")
    }

    /// A symlink at `<base>/<at>` pointing to `target`.
    fn link(&self, at: &str, target: &Path) {
        std::os::unix::fs::symlink(target, self.base.join(at)).expect("create a symlink");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// [`RealHost`] with its stdout recorded. Every other operation goes
/// straight to the real OS.
struct Recording {
    real: RealHost,
    stdout: String,
}

impl Host for Recording {
    fn write_stdout(&mut self, text: &str) -> std::io::Result<()> {
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
    fn canonicalize(&mut self, path: &Path) -> std::io::Result<PathBuf> {
        self.real.canonicalize(path)
    }
    fn is_symlink(&mut self, path: &Path) -> bool {
        self.real.is_symlink(path)
    }
    fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.real.open_root(path)
    }
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<RootHandle> {
        self.real.open_beneath(root, names)
    }
    fn read_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<String> {
        self.real.read_file_beneath(root, names)
    }
    fn read_file(&mut self, path: &Path) -> std::io::Result<String> {
        self.real.read_file(path)
    }
    fn write_file(
        &mut self,
        path: &Path,
        contents: &str,
        beneath: Option<&Path>,
    ) -> std::io::Result<()> {
        self.real.write_file(path, contents, beneath)
    }
    fn create_dir(&mut self, path: &Path, beneath: Option<&Path>) -> std::io::Result<()> {
        self.real.create_dir(path, beneath)
    }
    fn list_dir(&mut self, path: &Path, beneath: Option<&Path>) -> std::io::Result<Vec<String>> {
        self.real.list_dir(path, beneath)
    }
    fn printed(&self) -> Vec<String> {
        self.stdout.lines().map(str::to_string).collect()
    }
    fn take_output(&mut self) -> Output {
        Output {
            stdout: std::mem::take(&mut self.stdout),
            stderr: String::new(),
        }
    }
}

/// What a program prints when it reads `path` through `fs`, restricted
/// to `root` and then to each of `narrow` in turn: the file's text, or
/// `REFUSED: ` and the `IoError`'s message.
fn read(root: &Path, narrow: &[&str], path: &str) -> String {
    let narrowing: String = narrow
        .iter()
        .map(|dir| format!(".restrict_to(\"{dir}\")"))
        .collect();
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let local = fs.restrict_to(\"{}\"){narrowing}\n\
         \x20   match local.read_to_string(Path.new(\"{path}\")) {{\n\
         \x20       Ok(text) => {{\n\
         \x20           let _ = io.println(text)\n\
         \x20       }}\n\
         \x20       Err(e) => {{\n\
         \x20           let message = e.message\n\
         \x20           let _ = io.println(\"REFUSED: {{message}}\")\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n",
        root.display()
    );
    let mut host = Recording {
        real: RealHost::new(),
        stdout: String::new(),
    };
    let outcome = run(&src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout.trim_end().to_string()
}

/// Assert that reading `path` beneath `root` is refused with a message
/// containing `why`.
#[track_caller]
fn assert_refused(root: &Path, narrow: &[&str], path: &str, why: &str) {
    let printed = read(root, narrow, path);
    assert!(
        printed.starts_with("REFUSED: ") && printed.contains(why),
        "reading `{path}` should be refused for `{why}`, printed {printed:?}"
    );
}

// ---- Symlinks, at every component -----------------------------------------

#[test]
fn a_symlink_at_the_first_component_is_refused() {
    let sandbox = Sandbox::new("first");
    sandbox.link("root/out", &sandbox.outside());
    assert_refused(&sandbox.root(), &[], "out/secret.txt", "symlink");
}

#[test]
fn a_symlink_at_an_intermediate_component_is_refused() {
    let sandbox = Sandbox::new("intermediate");
    sandbox.link("root/sub/out", &sandbox.outside());
    assert_refused(&sandbox.root(), &[], "sub/out/secret.txt", "symlink");
}

#[test]
fn a_symlink_at_the_last_component_is_refused() {
    let sandbox = Sandbox::new("last");
    sandbox.link("root/sub/escape.txt", &sandbox.outside().join("secret.txt"));
    assert_refused(&sandbox.root(), &[], "sub/escape.txt", "symlink");
}

#[test]
fn a_dangling_symlink_is_refused() {
    let sandbox = Sandbox::new("dangling");
    sandbox.link("root/gone.txt", &sandbox.base.join("nothing-here"));
    sandbox.link("root/gone", &sandbox.base.join("nothing-here"));
    assert_refused(&sandbox.root(), &[], "gone.txt", "symlink");
    assert_refused(&sandbox.root(), &[], "gone/secret.txt", "symlink");
}

#[test]
fn a_symlink_pointing_inside_the_root_is_refused() {
    let sandbox = Sandbox::new("inside-link");
    sandbox.link("root/alias.txt", &sandbox.root().join("inside.txt"));
    sandbox.link("root/alias", &sandbox.root().join("sub"));
    sandbox.link("root/relative.txt", Path::new("sub/nested.txt"));
    assert_refused(&sandbox.root(), &[], "alias.txt", "symlink");
    assert_refused(&sandbox.root(), &[], "alias/nested.txt", "symlink");
    assert_refused(&sandbox.root(), &[], "relative.txt", "symlink");
}

// ---- `..`, absolute and empty paths ----------------------------------------

#[test]
fn dot_dot_inside_the_root_resolves() {
    let sandbox = Sandbox::new("dotdot-inside");
    assert_eq!(
        read(&sandbox.root(), &[], "sub/../inside.txt"),
        "inside text"
    );
    assert_eq!(
        read(&sandbox.root(), &[], "./sub/./../sub/nested.txt"),
        "nested text"
    );
    // `..` is resolved on the path as written, so it undoes a symlink
    // rather than climbing out of where the symlink points.
    sandbox.link("root/out", &sandbox.outside());
    assert_eq!(
        read(&sandbox.root(), &[], "out/../inside.txt"),
        "inside text"
    );
}

#[test]
fn dot_dot_above_the_root_is_refused() {
    let sandbox = Sandbox::new("dotdot-above");
    for path in [
        "../outside/secret.txt",
        "sub/../../outside/secret.txt",
        "..",
    ] {
        assert_refused(
            &sandbox.root(),
            &[],
            path,
            "outside the restricted directory",
        );
    }
    // Narrowed to `sub`, `..` may not climb back to the first root.
    assert_refused(
        &sandbox.root(),
        &["sub"],
        "../inside.txt",
        "outside the restricted directory",
    );
}

#[test]
fn absolute_and_empty_paths_are_refused() {
    let sandbox = Sandbox::new("absolute");
    let inside = sandbox.root().join("inside.txt");
    let outside = sandbox.outside().join("secret.txt");
    assert_refused(
        &sandbox.root(),
        &[],
        &inside.display().to_string(),
        "absolute",
    );
    assert_refused(
        &sandbox.root(),
        &[],
        &outside.display().to_string(),
        "absolute",
    );
    assert_refused(&sandbox.root(), &[], "", "empty");
}

// ---- The root -----------------------------------------------------------------

#[test]
fn a_symlinked_root_is_refused() {
    let sandbox = Sandbox::new("root-link");
    sandbox.link("root-link", &sandbox.root());
    for root in ["root-link", "root-link/", "root-link/."] {
        assert_refused(&sandbox.base.join(root), &[], "inside.txt", "symlink");
    }
}

#[test]
fn a_symlink_narrowed_to_is_refused() {
    let sandbox = Sandbox::new("narrow-link");
    sandbox.link("root/out", &sandbox.outside());
    sandbox.link("root/alias", &sandbox.root().join("sub"));
    assert_refused(&sandbox.root(), &["out"], "secret.txt", "symlink");
    assert_refused(&sandbox.root(), &["alias"], "nested.txt", "symlink");
}

#[test]
fn a_root_reached_through_a_symlinked_ancestor_is_kept() {
    // The root is trusted text: only its last component must not be a
    // symlink. On macOS the temp dir itself sits under one, `/var`.
    let sandbox = Sandbox::new("ancestor-link");
    sandbox.link("via", &sandbox.base);
    assert_eq!(
        read(&sandbox.base.join("via/root"), &[], "sub/nested.txt"),
        "nested text"
    );
}

#[test]
fn files_beneath_the_root_and_a_narrowed_root_are_read() {
    let sandbox = Sandbox::new("allowed");
    assert_eq!(read(&sandbox.root(), &[], "inside.txt"), "inside text");
    assert_eq!(read(&sandbox.root(), &[], "sub/nested.txt"), "nested text");
    assert_eq!(read(&sandbox.root(), &["sub"], "nested.txt"), "nested text");
    assert_eq!(
        read(&sandbox.root(), &["./sub/"], "nested.txt"),
        "nested text"
    );
}

#[test]
fn a_root_that_is_missing_or_not_a_directory_refuses_every_read() {
    let sandbox = Sandbox::new("bad-root");
    assert_refused(&sandbox.base.join("missing"), &[], "inside.txt", "missing");
    assert_refused(
        &sandbox.root().join("inside.txt"),
        &[],
        "inside.txt",
        "inside.txt",
    );
    assert_refused(&sandbox.root(), &["missing"], "nested.txt", "missing");
}

#[test]
fn a_relative_root_and_a_file_root_are_read() {
    // Relative to the working directory, which `cargo test` sets to this
    // crate's own directory.
    let marker = "Race-free confined reads (#94)";
    assert!(read(Path::new("tests"), &[], "confined_reads.rs").contains(marker));
    assert!(read(Path::new("tests/confined_reads.rs"), &[], ".").contains(marker));
    assert!(read(Path::new("."), &["tests"], "confined_reads.rs").contains(marker));
}

#[test]
fn a_fifo_is_refused_rather_than_waited_on() {
    // Opening a FIFO for reading blocks until a writer comes, so the walk
    // opens without blocking and then refuses anything not a regular file.
    let sandbox = Sandbox::new("fifo");
    let made = std::process::Command::new("mkfifo")
        .arg(sandbox.root().join("pipe"))
        .status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("skipped: needs `mkfifo` to make a FIFO");
        return;
    }
    assert_refused(&sandbox.root(), &[], "pipe", "not a regular file");
}
