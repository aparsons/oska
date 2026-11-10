//! File capabilities (#97, ADR 0008): `restrict_to_file` on `FsRead` and
//! `FsWrite` gives a `FileRead` or `FileWrite` for exactly one file, and
//! a directory capability hands out one for a file beneath it, from a
//! `Path` that may be untrusted.
//!
//! The API tests run Oska programs on [`FakeHost`], on every OS. The
//! escape tests run them on [`RealHost`] against a real temporary
//! directory with real symlinks (Unix only): a symlinked file name, a
//! symlinked parent, and a file or parent swapped after the capability
//! was made.

mod common;

use common::{assert_refused, run_fake};
use oska_run::{run, FakeHost};

/// Helpers every program below may call, to print what an operation
/// returned: its text, or `REFUSED: ` and its error's message.
const SHOWN: &str = r#"
fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// The source of a program whose `main` runs `body`, each line one
/// `io.println`. `main` is given `io: Stdout`, `read: FsRead`,
/// `write: FsWrite`, and `args: Args`.
fn program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, read: FsRead, write: FsWrite, args: Args) \
         -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{SHOWN}"
    )
}

/// What `body` prints on `host`, with `args` after the program path. The
/// host is left as the program left it.
fn printed_with(host: &mut FakeHost, args: &[&str], body: &str) -> String {
    let src = program(body);
    let args: Vec<String> = std::iter::once("prog.osk")
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    let outcome = run(&src, host, &args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// What `body` prints on `host`.
fn printed(mut host: FakeHost, body: &str) -> String {
    printed_with(&mut host, &[], body)
}

// ---- Reading and writing one file ----------------------------------------------------

#[test]
fn a_file_read_reads_its_file_with_no_path() {
    let host = FakeHost::new()
        .with_file("README.md", "# Oska")
        .with_file("docs/intro.md", "# Intro");
    let out = printed(
        host,
        r#"    let readme = read.restrict_to_file("README.md")
    io.println(shown(readme.read_to_string()))?
    io.println(shown(read.restrict_to_file("docs/intro.md").read_to_string()))?
    io.println(shown(readme.read_to_string()))?"#,
    );
    assert_eq!(out, "# Oska\n# Intro\n# Oska\n");
}

#[test]
fn a_file_write_creates_and_replaces_its_file_with_no_path() {
    let mut host = FakeHost::new()
        .with_dir("out")
        .with_file("other.txt", "other");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let file = write.restrict_to_file("out/result.txt")
    io.println(done(file.write("first draft")))?
    io.println(done(file.write("final")))?"#,
    );
    assert_eq!(out, "done\ndone\n");
    assert_eq!(host.file("out/result.txt"), Some("final"));
    assert_eq!(host.file("other.txt"), Some("other"));
}

#[test]
fn a_missing_file_is_an_error_to_read_and_is_created_by_a_write() {
    let mut host = FakeHost::new().with_dir("out");
    let out = printed_with(
        &mut host,
        &[],
        r#"    io.println(shown(read.restrict_to_file("out/new.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("out/new.txt").write("made")))?
    io.println(shown(read.restrict_to_file("out/new.txt").read_to_string()))?"#,
    );
    assert!(
        out.starts_with("REFUSED: ") && out.ends_with("\ndone\nmade\n"),
        "{out:?}"
    );
}

// ---- A directory capability hands out a file ------------------------------------------

#[test]
fn a_directory_capability_hands_out_a_file_beneath_it() {
    let mut host = FakeHost::new()
        .with_file("docs/intro.md", "# Intro")
        .with_file("docs/adr/0001.md", "# One")
        .with_dir("site");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let docs = read.restrict_to("docs")
    io.println(shown(docs.restrict_to_file(Path.new("intro.md")).read_to_string()))?
    io.println(shown(docs.restrict_to_file(Path.new("adr/../adr/0001.md")).read_to_string()))?
    let site = write.restrict_to("site")
    io.println(done(site.restrict_to_file(Path.new("index.html")).write("<p>hi</p>")))?"#,
    );
    assert_eq!(out, "# Intro\n# One\ndone\n");
    assert_eq!(host.file("site/index.html"), Some("<p>hi</p>"));
}

#[test]
fn a_file_named_by_untrusted_text_stays_beneath_the_directory() {
    let mut host = FakeHost::new()
        .with_file("uploads/avatar.png", "pixels")
        .with_file("secret.txt", "secret");
    let out = printed_with(
        &mut host,
        &["avatar.png", "../secret.txt", "/secret.txt", "", "."],
        r#"    let uploads = read.restrict_to("uploads")
    for i in 1..6 {
        let name = args.get(i).unwrap_or("")
        io.println(shown(uploads.restrict_to_file(Path.from(name)).read_to_string()))?
    }"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "pixels");
    assert!(
        lines[1].starts_with("REFUSED: cannot restrict to `../secret.txt`: ")
            && lines[1].contains("leads outside the restricted directory"),
        "{out:?}"
    );
    assert!(
        lines[2].starts_with("REFUSED: cannot restrict to `/secret.txt`: ")
            && lines[2].contains("is absolute"),
        "{out:?}"
    );
    assert!(
        lines[3].starts_with("REFUSED: cannot restrict to ``: ") && lines[3].contains("empty path"),
        "{out:?}"
    );
    assert!(
        lines[4].starts_with("REFUSED: cannot restrict to `.`: ")
            && lines[4].contains("does not end in a file name"),
        "{out:?}"
    );
}

#[test]
fn a_write_file_beneath_a_directory_cannot_leave_it() {
    let mut host = FakeHost::new()
        .with_dir("site")
        .with_file("secret.txt", "secret");
    let out = printed_with(
        &mut host,
        &["../secret.txt"],
        r#"    let site = write.restrict_to("site")
    let name = args.get(1).unwrap_or("")
    io.println(done(site.restrict_to_file(Path.from(name)).write("pwned")))?"#,
    );
    assert!(
        out.starts_with("REFUSED: cannot restrict to `../secret.txt`: "),
        "{out:?}"
    );
    assert_eq!(host.file("secret.txt"), Some("secret"));
}

// ---- Broken roots ---------------------------------------------------------------------

#[test]
fn a_missing_parent_fails_every_operation_naming_the_file() {
    let mut host = FakeHost::new().with_file("docs/intro.md", "# Intro");
    let out = printed_with(
        &mut host,
        &[],
        r#"    io.println(shown(read.restrict_to_file("nodir/x.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("nodir/x.txt").write("x")))?
    let docs = read.restrict_to("docs")
    io.println(shown(docs.restrict_to_file(Path.new("nodir/x.txt")).read_to_string()))?"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out:?}");
    assert!(lines[0].starts_with("REFUSED: cannot restrict to `nodir/x.txt`: "));
    assert!(lines[1].starts_with("REFUSED: cannot restrict to `nodir/x.txt`: "));
    assert!(lines[2].starts_with("REFUSED: cannot restrict to `nodir/x.txt`: "));
    assert_eq!(host.files_under("/").len(), 1, "nothing was written");
}

#[test]
fn a_name_that_is_not_a_file_fails_every_operation() {
    let mut host = FakeHost::new().with_file("docs/intro.md", "# Intro");
    let out = printed_with(
        &mut host,
        &[],
        r#"    io.println(shown(read.restrict_to_file("docs").read_to_string()))?
    io.println(done(write.restrict_to_file("docs").write("x")))?"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out:?}");
    assert!(lines.iter().all(|l| l.starts_with("REFUSED: ")), "{out:?}");
    assert_eq!(host.files_under("/").len(), 1, "nothing was written");
}

#[test]
fn restrict_to_with_a_file_root_gives_a_broken_directory_capability() {
    let mut host = FakeHost::new()
        .with_file("docs/intro.md", "# Intro")
        .with_dir("out");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let docs = read.restrict_to("docs/intro.md")
    io.println(shown(docs.read_to_string(Path.new("."))))?
    io.println(shown(docs.read_to_string(Path.new("intro.md"))))?
    let narrowed = read.restrict_to("docs").restrict_to(Path.new("intro.md"))
    io.println(shown(narrowed.read_to_string(Path.new("."))))?
    let site = write.restrict_to("docs/intro.md")
    io.println(done(site.write(Path.new("x.txt"), "x")))?"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{out:?}");
    for line in &lines {
        assert!(
            line.starts_with("REFUSED: cannot restrict to `")
                && line.ends_with("is not a directory"),
            "{out:?}"
        );
    }
    assert_eq!(host.files_under("/docs").len(), 1, "nothing was written");
}

// ---- Symlinks -------------------------------------------------------------------------

#[test]
fn a_file_capability_never_follows_a_symlinked_file_name() {
    let mut host = FakeHost::new()
        .with_file("secret.txt", "secret")
        .with_symlink("docs/link.md", "secret.txt")
        .with_symlink("docs/dangling.md", "nothing.txt");
    let out = printed_with(
        &mut host,
        &[],
        r#"    io.println(shown(read.restrict_to_file("docs/link.md").read_to_string()))?
    io.println(done(write.restrict_to_file("docs/link.md").write("pwned")))?
    io.println(done(write.restrict_to_file("docs/dangling.md").write("pwned")))?
    let docs = write.restrict_to("docs")
    io.println(done(docs.restrict_to_file(Path.new("link.md")).write("pwned")))?"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{out:?}");
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("REFUSED: ") && l.contains("symlink")),
        "{out:?}"
    );
    assert_eq!(host.file("secret.txt"), Some("secret"));
    assert_eq!(host.file("nothing.txt"), None);
}

#[test]
fn a_file_beneath_a_symlinked_directory_is_refused() {
    let mut host = FakeHost::new()
        .with_file("outside/secret.txt", "secret")
        .with_file("root/inside.txt", "inside")
        .with_symlink("root/out", "outside");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let root = read.restrict_to("root")
    io.println(shown(root.restrict_to_file(Path.new("out/secret.txt")).read_to_string()))?
    let site = write.restrict_to("root")
    io.println(done(site.restrict_to_file(Path.new("out/planted.txt")).write("pwned")))?"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out:?}");
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("REFUSED: cannot restrict to `out/") && l.contains("symlink")),
        "{out:?}"
    );
    assert_eq!(host.file("outside/planted.txt"), None);
}

// ---- Granting -------------------------------------------------------------------------

#[test]
fn main_cannot_be_granted_a_file_capability() {
    for ty in ["FileRead", "FileWrite"] {
        let src = format!("fn main(file: {ty}) {{\n}}\n");
        assert_refused(&src, "E0370");
        let outcome = run_fake(&src, &[]);
        assert_eq!(
            outcome.diagnostics[0].message,
            format!(
                "`oskac run` cannot grant `{ty}`: only attenuation makes a file capability, \
                 with `restrict_to_file` on `FsRead`, `FsWrite` or a directory capability"
            )
        );
    }
}

// ---- Confinement on the real host -----------------------------------------------------

/// A real temporary directory with real symlinks. A program's results go
/// to stdout, which a recording host captures; a hook runs after a file
/// capability is made, to swap the filesystem under it.
#[cfg(unix)]
mod real {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use oska_run::{run, Host, Output, RealHost, RootHandle};

    use super::program;

    /// A fresh directory under the system temp dir, removed on drop:
    ///
    /// ```text
    /// root/inside.txt         "inside text"
    /// root/sub/nested.txt     "nested text"
    /// outside/secret.txt      "outside text"
    /// ```
    pub struct Sandbox {
        base: PathBuf,
    }

    impl Sandbox {
        pub fn new(test: &str) -> Sandbox {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let unique = format!(
                "oska-filecap-{test}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let base = std::env::temp_dir().join(unique);
            for dir in ["root/sub", "outside"] {
                std::fs::create_dir_all(base.join(dir)).expect("create a directory");
            }
            std::fs::write(base.join("root/inside.txt"), "inside text").expect("write inside");
            std::fs::write(base.join("root/sub/nested.txt"), "nested text").expect("write nested");
            std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write outside");
            let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
            Sandbox { base }
        }

        /// The absolute path of `at` beneath the sandbox.
        pub fn path(&self, at: &str) -> PathBuf {
            self.base.join(at)
        }

        /// A symlink at `<base>/<at>` pointing to `<base>/<target>`.
        pub fn link(&self, at: &str, target: &str) {
            std::os::unix::fs::symlink(self.path(target), self.path(at)).expect("create a symlink");
        }

        /// The text of the regular file at `at`, if there is one.
        pub fn file(&self, at: &str) -> Option<String> {
            let path = self.path(at);
            let there = std::fs::symlink_metadata(&path).ok()?;
            there
                .is_file()
                .then(|| std::fs::read_to_string(path).expect("read"))
        }

        /// What `body` printed, with `{root}` and `{outside}` replaced by
        /// the sandbox's directories. `args` follow the program path.
        pub fn printed(&self, args: &[&str], body: &str) -> String {
            self.printed_swapping(args, body, || {})
        }

        /// As [`Sandbox::printed`], with `swap` run once, right after the
        /// program makes its first file capability.
        pub fn printed_swapping(
            &self,
            args: &[&str],
            body: &str,
            swap: impl FnOnce() + Send + 'static,
        ) -> String {
            let body = body
                .replace("{root}", &self.path("root").display().to_string())
                .replace("{outside}", &self.path("outside").display().to_string());
            let src = program(&body);
            let mut host = Recording {
                real: RealHost::new(),
                stdout: String::new(),
                after_file_root: Some(Box::new(swap)),
            };
            let args: Vec<String> = std::iter::once("prog.osk")
                .chain(args.iter().copied())
                .map(str::to_string)
                .collect();
            let outcome = run(&src, &mut host, &args);
            assert!(
                outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
                "{src} should run: {outcome:#?}"
            );
            outcome.stdout
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// [`RealHost`] with its stdout recorded, and a hook run after the
    /// first file capability is made. Every other operation goes straight
    /// to the real OS.
    struct Recording {
        real: RealHost,
        stdout: String,
        after_file_root: Option<Box<dyn FnOnce() + Send>>,
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
        fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
            self.real.open_root(path)
        }
        fn open_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<RootHandle> {
            self.real.open_beneath(root, names)
        }
        fn open_file_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
            let opened = self.real.open_file_root(path);
            if let Some(swap) = self.after_file_root.take() {
                swap();
            }
            opened
        }
        fn open_file_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<RootHandle> {
            let opened = self.real.open_file_beneath(root, names);
            if let Some(swap) = self.after_file_root.take() {
                swap();
            }
            opened
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
        fn write_file(&mut self, path: &Path, contents: &str) -> std::io::Result<()> {
            self.real.write_file(path, contents)
        }
        fn write_file_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
            contents: &str,
        ) -> std::io::Result<()> {
            self.real.write_file_beneath(root, names, contents)
        }
        fn create_dir(&mut self, path: &Path) -> std::io::Result<()> {
            self.real.create_dir(path)
        }
        fn create_dir_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<()> {
            self.real.create_dir_beneath(root, names)
        }
        fn list_dir(&mut self, path: &Path) -> std::io::Result<Vec<String>> {
            self.real.list_dir(path)
        }
        fn list_dir_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<Vec<String>> {
            self.real.list_dir_beneath(root, names)
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

    /// Assert that every line of `printed` is a refusal containing `why`,
    /// and that there are `lines` of them.
    #[track_caller]
    pub fn assert_all_refused(printed: &str, lines: usize, why: &str) {
        assert_eq!(printed.lines().count(), lines, "{printed:?}");
        for line in printed.lines() {
            assert!(
                line.starts_with("REFUSED: ") && line.contains(why),
                "should be refused for `{why}`: {printed:?}"
            );
        }
    }

    // ---- Reading and writing ------------------------------------------------------

    #[test]
    fn file_capabilities_read_and_write_their_file() {
        let sandbox = Sandbox::new("round-trip");
        let out = sandbox.printed(
            &[],
            r#"    let inside = read.restrict_to_file("{root}/inside.txt")
    io.println(shown(inside.read_to_string()))?
    let sub = read.restrict_to("{root}")
    io.println(shown(sub.restrict_to_file(Path.new("sub/nested.txt")).read_to_string()))?
    let out = write.restrict_to_file("{root}/new.txt")
    io.println(done(out.write("a long first draft")))?
    io.println(done(out.write("short")))?"#,
        );
        assert_eq!(out, "inside text\nnested text\ndone\ndone\n");
        assert_eq!(sandbox.file("root/new.txt").as_deref(), Some("short"));
    }

    // ---- A symlinked file name ----------------------------------------------------

    #[test]
    fn a_symlinked_file_name_is_refused_and_nothing_is_written_through_it() {
        let sandbox = Sandbox::new("name-link");
        sandbox.link("root/link.txt", "outside/secret.txt");
        sandbox.link("root/sub/link.txt", "outside/secret.txt");
        sandbox.link("root/alias.txt", "root/inside.txt");
        sandbox.link("root/dangling.txt", "outside/planted.txt");
        let first_roots: String = ["link.txt", "alias.txt", "dangling.txt"]
            .iter()
            .map(|name| {
                format!(
                    "    io.println(shown(read.restrict_to_file(\"{{root}}/{name}\").read_to_string()))?\n\
                     \x20   io.println(done(write.restrict_to_file(\"{{root}}/{name}\").write(\"pwned\")))?\n"
                )
            })
            .collect();
        let out = sandbox.printed(
            &["link.txt", "sub/link.txt", "alias.txt", "dangling.txt"],
            &format!(
                "{first_roots}\
                 \x20   let readable = read.restrict_to(\"{{root}}\")\n\
                 \x20   let writable = write.restrict_to(\"{{root}}\")\n\
                 \x20   for i in 1..5 {{\n\
                 \x20       let name = args.get(i).unwrap_or(\"\")\n\
                 \x20       io.println(shown(readable.restrict_to_file(Path.from(name)).read_to_string()))?\n\
                 \x20       io.println(done(writable.restrict_to_file(Path.from(name)).write(\"pwned\")))?\n\
                 \x20   }}"
            ),
        );
        assert_all_refused(&out, 14, "symlink");
        assert_eq!(
            sandbox.file("outside/secret.txt").as_deref(),
            Some("outside text")
        );
        assert_eq!(sandbox.file("outside/planted.txt"), None);
        assert_eq!(
            sandbox.file("root/inside.txt").as_deref(),
            Some("inside text")
        );
    }

    // ---- A symlinked parent -------------------------------------------------------

    #[test]
    fn a_file_beneath_a_symlinked_directory_is_refused_at_every_position() {
        let sandbox = Sandbox::new("parent-link");
        sandbox.link("root/out", "outside");
        sandbox.link("root/sub/out", "outside");
        sandbox.link("root/alias", "root/sub");
        let out = sandbox.printed(
            &["out/secret.txt", "sub/out/secret.txt", "alias/nested.txt"],
            r#"    let readable = read.restrict_to("{root}")
    let writable = write.restrict_to("{root}")
    for i in 1..4 {
        let name = args.get(i).unwrap_or("")
        io.println(shown(readable.restrict_to_file(Path.from(name)).read_to_string()))?
        io.println(done(writable.restrict_to_file(Path.from(name)).write("pwned")))?
    }
    io.println(done(writable.restrict_to_file(Path.new("out/planted.txt")).write("pwned")))?"#,
        );
        assert_all_refused(&out, 7, "symlink");
        assert_eq!(sandbox.file("outside/planted.txt"), None);
        assert_eq!(
            sandbox.file("outside/secret.txt").as_deref(),
            Some("outside text")
        );
        assert_eq!(
            sandbox.file("root/sub/nested.txt").as_deref(),
            Some("nested text")
        );
    }

    // ---- Swapped after the capability was made ------------------------------------

    #[test]
    fn a_file_swapped_for_a_symlink_after_the_capability_was_made_is_refused() {
        let sandbox = Sandbox::new("swap-file");
        let (file, secret) = (
            sandbox.path("root/inside.txt"),
            sandbox.path("outside/secret.txt"),
        );
        let out = sandbox.printed_swapping(
            &[],
            r#"    let inside = read.restrict_to_file("{root}/inside.txt")
    io.println(shown(inside.read_to_string()))?
    io.println(shown(inside.read_to_string()))?
    let out = write.restrict_to_file("{root}/inside.txt")
    io.println(done(out.write("pwned")))?"#,
            move || {
                std::fs::remove_file(&file).expect("remove the file");
                std::os::unix::fs::symlink(&secret, &file).expect("swap in a symlink");
            },
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out:?}");
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("REFUSED: ") && l.contains("symlink")),
            "{out:?}"
        );
        assert_eq!(
            sandbox.file("outside/secret.txt").as_deref(),
            Some("outside text")
        );
    }

    /// A swap of `root` for a symlink to `outside`, which holds a decoy
    /// `inside.txt`; the real `root` is moved to `root-moved`.
    fn swap_parent(sandbox: &Sandbox) -> impl FnOnce() + Send + 'static {
        std::fs::write(sandbox.path("outside/inside.txt"), "decoy text").expect("write decoy");
        let (root, moved, outside) = (
            sandbox.path("root"),
            sandbox.path("root-moved"),
            sandbox.path("outside"),
        );
        move || {
            std::fs::rename(&root, &moved).expect("move the parent away");
            std::os::unix::fs::symlink(&outside, &root).expect("swap in a symlink");
        }
    }

    #[test]
    fn a_file_read_whose_parent_was_swapped_for_a_symlink_still_reads_its_own_file() {
        let sandbox = Sandbox::new("swap-parent-read");
        let swap = swap_parent(&sandbox);
        let out = sandbox.printed_swapping(
            &[],
            r#"    let inside = read.restrict_to_file("{root}/inside.txt")
    io.println(shown(inside.read_to_string()))?
    io.println(shown(inside.read_to_string()))?"#,
            swap,
        );
        // The capability holds the parent it was made with, so it never
        // meets the decoy now at the same path.
        assert_eq!(out, "inside text\ninside text\n");
    }

    #[test]
    fn a_file_write_whose_parent_was_swapped_for_a_symlink_still_writes_its_own_file() {
        let sandbox = Sandbox::new("swap-parent-write");
        let swap = swap_parent(&sandbox);
        let out = sandbox.printed_swapping(
            &[],
            r#"    let out = write.restrict_to_file("{root}/inside.txt")
    io.println(done(out.write("rewritten")))?
    io.println(done(out.write("again")))?"#,
            swap,
        );
        assert_eq!(out, "done\ndone\n");
        assert_eq!(
            sandbox.file("outside/inside.txt").as_deref(),
            Some("decoy text")
        );
        assert_eq!(
            sandbox.file("root-moved/inside.txt").as_deref(),
            Some("again")
        );
    }

    // ---- Broken roots -------------------------------------------------------------

    #[test]
    fn a_missing_parent_or_a_name_that_is_not_a_file_fails_every_operation() {
        let sandbox = Sandbox::new("broken");
        let out = sandbox.printed(
            &[],
            r#"    io.println(shown(read.restrict_to_file("{root}/nodir/x.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("{root}/nodir/x.txt").write("x")))?
    io.println(shown(read.restrict_to_file("{root}/sub").read_to_string()))?
    io.println(done(write.restrict_to_file("{root}/sub").write("x")))?
    io.println(shown(read.restrict_to_file("{root}/inside.txt/x").read_to_string()))?"#,
        );
        assert_all_refused(&out, 5, "");
        let sub = out.lines().nth(2).unwrap_or_default();
        assert!(!sub.contains("cannot restrict to"), "{out:?}");
        assert!(!sandbox.path("root/nodir").exists());
        assert!(sandbox.path("root/sub").is_dir());
    }

    #[test]
    fn restrict_to_with_a_file_root_gives_a_broken_directory_capability() {
        let sandbox = Sandbox::new("file-as-dir");
        let out = sandbox.printed(
            &[],
            r#"    let docs = read.restrict_to("{root}/inside.txt")
    io.println(shown(docs.read_to_string(Path.new("."))))?
    io.println(shown(docs.read_to_string(Path.new("inside.txt"))))?
    let narrowed = read.restrict_to("{root}").restrict_to(Path.new("inside.txt"))
    io.println(shown(narrowed.read_to_string(Path.new("."))))?
    let site = write.restrict_to("{root}/inside.txt")
    io.println(done(site.write(Path.new("x.txt"), "x")))?"#,
        );
        assert_all_refused(&out, 4, "cannot restrict to `");
        for line in out.lines() {
            assert!(line.contains("not a directory"), "{out:?}");
        }
        assert_eq!(
            sandbox.file("root/inside.txt").as_deref(),
            Some("inside text")
        );
    }
}
