//! What the site generator needs from Oska (#74): `FsWrite.create_dir`,
//! `restrict_to` naming a single file, and `String.to_lowercase`.
//!
//! The confinement tests run on [`RealHost`] against a real temporary
//! directory, with real symlinks; the rest run on [`FakeHost`].

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{assert_runs, run_fake, run_on};
use oska_run::{run, FakeHost, Host, RealHost, RunOutcome};

/// Run `src` on `host`, which the test inspects afterwards.
fn run_kept(src: &str, host: &mut FakeHost) -> RunOutcome {
    run(src, host, &["prog.osk".to_string()])
}

/// Assert that a program that checks ran, and that `main` returned the
/// `Err` of refusing to create `path`.
fn assert_refused_at_runtime(outcome: &RunOutcome, path: &str) {
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "creating `{path}` should run: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, 1, "creating `{path}`: {outcome:#?}");
}

// ---- create_dir on the fake host -------------------------------------------

#[test]
fn create_dir_makes_a_directory_that_a_restriction_and_a_write_can_use() {
    let src = "fn main(fs: FsWrite) -> Result<(), IoError> {
    fs.create_dir(Path.new(\"_site\"))?
    let site = fs.restrict_to(\"_site\")
    site.create_dir(Path.new(\"adr\"))?
    site.write(Path.new(\"adr/one.html\"), \"one\")
}
";
    let mut host = FakeHost::new();
    let outcome = run_kept(src, &mut host);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(host.file("_site/adr/one.html"), Some("one"));
}

#[test]
fn create_dir_of_a_directory_that_exists_succeeds_and_keeps_its_files() {
    let src = "fn main(fs: FsWrite) -> Result<(), IoError> {
    fs.create_dir(Path.new(\"_site\"))?
    fs.create_dir(Path.new(\"_site\"))
}
";
    let mut host = FakeHost::new().with_file("_site/old.html", "old");
    let outcome = run_kept(src, &mut host);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(host.file("_site/old.html"), Some("old"));
}

#[test]
fn create_dir_where_a_file_is_or_no_parent_is_an_error() {
    let src = "fn main(fs: FsWrite) -> Result<(), IoError> {
    fs.create_dir(Path.new(\"{path}\"))
}
";
    for path in ["taken", "missing/child", ""] {
        let src = src.replace("{path}", path);
        let host = FakeHost::new().with_file("taken", "a file");
        let outcome = run_on(host, &src, &[]);
        assert_refused_at_runtime(&outcome, path);
    }
}

#[test]
fn create_dir_refuses_a_path_out_of_its_root() {
    let src = "fn main(fs: FsWrite) -> Result<(), IoError> {
    fs.restrict_to(\"_site\").create_dir(Path.new(\"{path}\"))
}
";
    for path in ["..", "../out", "a/../../out", "/out", "", ".", "new/"] {
        let src = src.replace("{path}", path);
        let mut host = FakeHost::new().with_dir("_site");
        let outcome = run_kept(&src, &mut host);
        assert_refused_at_runtime(&outcome, path);
        assert!(
            host.list_dir(Path::new("/")).expect("the root") == ["_site"],
            "creating `{path}` made something outside `_site`"
        );
    }
}

#[test]
fn a_read_only_filesystem_cannot_create_a_directory() {
    let src = "fn main(fs: FsRead) {\n    let _ = fs.create_dir(Path.new(\"x\"))\n}\n";
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.iter().any(|d| d.code == "E0228"),
        "{outcome:#?}"
    );
}

// ---- create_dir confinement on the real host ---------------------------------

/// A fresh directory under the system temp dir, removed on drop. It holds
/// `root`, the directory a program is confined to, with a file and a
/// subdirectory, and `outside`, a sibling it must never touch.
struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-run-site-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("root/sub")).expect("create the root");
        std::fs::create_dir_all(base.join("outside")).expect("create the outside");
        std::fs::write(base.join("root/file.txt"), "inside").expect("write a file");
        std::fs::write(base.join("outside/secret.txt"), "secret").expect("write outside");
        let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
        Sandbox { base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn outside(&self) -> PathBuf {
        self.base.join("outside")
    }

    fn link(&self, name: &str, target: &Path) {
        std::os::unix::fs::symlink(target, self.root().join(name)).expect("create a symlink");
    }

    /// The names in `outside`, which nothing may add to.
    fn outside_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.outside())
            .expect("list the outside")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The exit code of a program that creates the directory `path` through
/// a filesystem restricted to `root`, on a bare [`RealHost`]: 0 when it
/// was created or already there, 1 when it was refused.
fn create_confined(root: &Path, path: &str) -> i32 {
    let src = format!(
        "fn main(fs: FsWrite) -> Result<(), IoError> {{\n\
         \x20   fs.restrict_to(\"{}\").create_dir(Path.new(\"{path}\"))\n\
         }}\n",
        root.display()
    );
    let outcome = run(&src, &mut RealHost::new(), &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src} should run: {outcome:#?}"
    );
    outcome.exit_code
}

#[test]
fn real_host_creates_a_directory_inside_the_root() {
    let sandbox = Sandbox::new("create");
    assert_eq!(create_confined(&sandbox.root(), "new"), 0);
    assert_eq!(create_confined(&sandbox.root(), "sub/deeper"), 0);
    assert_eq!(create_confined(&sandbox.root(), "sub/../again"), 0);
    for made in ["new", "sub/deeper", "again"] {
        assert!(sandbox.root().join(made).is_dir(), "`{made}` was not made");
    }
}

#[test]
fn real_host_keeps_a_directory_that_is_already_there() {
    let sandbox = Sandbox::new("create-again");
    std::fs::write(sandbox.root().join("sub/kept.txt"), "kept").expect("write");
    assert_eq!(create_confined(&sandbox.root(), "sub"), 0);
    assert_eq!(
        std::fs::read_to_string(sandbox.root().join("sub/kept.txt")).expect("kept"),
        "kept"
    );
}

#[test]
fn real_host_refuses_to_create_over_a_file_or_without_a_parent() {
    let sandbox = Sandbox::new("create-blocked");
    assert_eq!(create_confined(&sandbox.root(), "file.txt"), 1);
    assert_eq!(create_confined(&sandbox.root(), "missing/child"), 1);
    assert!(sandbox.root().join("file.txt").is_file());
    assert!(!sandbox.root().join("missing").exists());
}

#[test]
fn real_host_refuses_to_create_out_of_the_root() {
    let sandbox = Sandbox::new("create-escape");
    let absolute = sandbox.outside().join("abs").display().to_string();
    for path in [
        "..",
        "../outside/new",
        "sub/../../outside/new",
        absolute.as_str(),
        "",
        ".",
        "sub/..",
        "new/",
        "sub/.",
    ] {
        assert_eq!(
            create_confined(&sandbox.root(), path),
            1,
            "creating `{path}`"
        );
    }
    assert_eq!(sandbox.outside_names(), ["secret.txt"]);
    assert!(!sandbox.root().join("new").exists());
}

#[test]
fn real_host_never_creates_through_a_symlink() {
    let sandbox = Sandbox::new("create-symlink");
    sandbox.link("out", &sandbox.outside());
    sandbox.link("dangling", &sandbox.outside().join("planted"));
    sandbox.link("alias", &sandbox.root().join("sub"));
    // Through a directory symlink that leads out of the root.
    assert_eq!(create_confined(&sandbox.root(), "out/new"), 1);
    // At a dangling symlink that points out of the root.
    assert_eq!(create_confined(&sandbox.root(), "dangling"), 1);
    // At a symlink to a directory inside the root: never followed.
    assert_eq!(create_confined(&sandbox.root(), "alias"), 1);
    assert_eq!(sandbox.outside_names(), ["secret.txt"]);
}

#[test]
fn real_host_unrestricted_keeps_a_symlink_to_a_directory() {
    // Unrestricted, `create_dir` follows a symlink to a directory that is
    // already there. Restricted, it never does (`confined_writes.rs`).
    let sandbox = Sandbox::new("create-host");
    let root = sandbox.root();
    sandbox.link("alias", &root.join("sub"));
    assert!(RealHost::new().create_dir(&root.join("alias")).is_ok());
}

// ---- to_lowercase ----------------------------------------------------------------

#[test]
fn to_lowercase_lowers_every_letter_and_keeps_the_rest() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {
    io.println(\"Untrusted DATA, Ünïcode & ΣΑΣ: 42!\".to_lowercase())
}
";
    assert_runs(src, &[], "untrusted data, ünïcode & σας: 42!\n");
}
