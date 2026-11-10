//! Directory capabilities (#96, ADR 0008): `restrict_to` on `Fs`, `FsRead`
//! and `FsWrite` gives a `Dir`, `DirRead` or `DirWrite` that reaches only
//! beneath its root, and narrows further with a `Path` that may come from
//! untrusted text.
//!
//! The API tests run Oska programs on [`FakeHost`]. The confinement of a
//! narrowing from untrusted text runs on [`RealHost`] against a real
//! temporary directory with real symlinks (Unix only).

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

fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// What `main` prints on `host` when its body is `body`, each line one
/// `io.println`. `main` is given `io: Stdout`, `fs: Fs`, `read: FsRead`,
/// `write: FsWrite`, and `args: Args` holding `args` after the program
/// path. The host is left as the program left it.
fn printed_with(host: &mut FakeHost, args: &[&str], body: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: Fs, read: FsRead, write: FsWrite, args: Args) \
         -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{SHOWN}"
    );
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

/// What `body` prints on `host`, with no arguments.
fn printed(mut host: FakeHost, body: &str) -> String {
    printed_with(&mut host, &[], body)
}

// ---- Reading -----------------------------------------------------------------------------

#[test]
fn a_dir_read_reads_a_file_beneath_its_root() {
    let host = FakeHost::new()
        .with_file("docs/intro.md", "# Intro")
        .with_file("secret.txt", "secret");
    let out = printed(
        host,
        r#"    let docs = read.restrict_to("docs")
    io.println(shown(docs.read_to_string(Path.new("intro.md"))))?
    io.println(shown(docs.read_to_string(Path.new("../secret.txt"))))?"#,
    );
    assert_eq!(
        out,
        "# Intro\nREFUSED: `../secret.txt` leads outside the restricted directory\n"
    );
}

#[test]
fn a_dir_read_lists_a_directory_beneath_its_root() {
    let host = FakeHost::new()
        .with_file("docs/b.md", "")
        .with_file("docs/a.md", "")
        .with_file("docs/adr/0001.md", "")
        .with_file("other/x.md", "");
    let out = printed(
        host,
        r#"    let docs = read.restrict_to("docs")
    io.println(listed(docs.list_dir(Path.new("."))))?
    io.println(listed(docs.list_dir(Path.new("adr"))))?
    io.println(listed(docs.list_dir(Path.new("../other"))))?"#,
    );
    assert_eq!(
        out,
        "a.md,adr,b.md\n0001.md\nREFUSED: `../other` leads outside the restricted directory\n"
    );
}

// ---- Writing -----------------------------------------------------------------------------

#[test]
fn a_dir_write_writes_and_creates_directories_only_beneath_its_root() {
    let mut host = FakeHost::new().with_dir("_site");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let site = write.restrict_to("_site")
    io.println(done(site.create_dir(Path.new("adr"))))?
    io.println(done(site.write(Path.new("adr/index.html"), "<p>adr</p>")))?
    io.println(done(site.write(Path.new("../escaped.html"), "x")))?
    io.println(done(site.create_dir(Path.new("../escaped"))))?"#,
    );
    assert_eq!(
        out,
        "done\ndone\n\
         REFUSED: `../escaped.html` leads outside the restricted directory\n\
         REFUSED: `../escaped` leads outside the restricted directory\n"
    );
    assert_eq!(host.file("_site/adr/index.html"), Some("<p>adr</p>"));
    assert_eq!(host.file("escaped.html"), None);
}

#[test]
fn a_dir_reads_through_read_only_and_writes_through_write_only() {
    let mut host = FakeHost::new().with_dir("app").with_file("other.txt", "");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let app = fs.restrict_to("app")
    io.println(done(app.write_only().write(Path.new("state.txt"), "saved")))?
    io.println(shown(app.read_only().read_to_string(Path.new("state.txt"))))?
    io.println(done(app.write_only().write(Path.new("../other.txt"), "x")))?"#,
    );
    assert_eq!(
        out,
        "done\nsaved\nREFUSED: `../other.txt` leads outside the restricted directory\n"
    );
    assert_eq!(host.file("app/state.txt"), Some("saved"));
    assert_eq!(host.file("other.txt"), Some(""));
}

// ---- Narrowing ---------------------------------------------------------------------------

#[test]
fn a_narrowing_from_untrusted_text_reaches_only_its_subdirectory() {
    let mut host = FakeHost::new()
        .with_file("uploads/alice/avatar.txt", "alice's")
        .with_file("uploads/bob/avatar.txt", "bob's")
        .with_dir("uploads/carol");
    let out = printed_with(
        &mut host,
        &["alice", "carol"],
        r#"    let uploads = fs.restrict_to("uploads")
    let reader = args.get(1).unwrap_or("")
    let writer = args.get(2).unwrap_or("")
    let mine = uploads.read_only().restrict_to(Path.from(reader))
    io.println(shown(mine.read_to_string(Path.new("avatar.txt"))))?
    io.println(shown(mine.read_to_string(Path.new("../bob/avatar.txt"))))?
    let theirs = uploads.write_only().restrict_to(Path.from(writer))
    io.println(done(theirs.write(Path.new("avatar.txt"), "carol's")))?
    io.println(done(theirs.write(Path.new("../bob/avatar.txt"), "pwned")))?"#,
    );
    assert_eq!(
        out,
        "alice's\n\
         REFUSED: `../bob/avatar.txt` leads outside the restricted directory\n\
         done\n\
         REFUSED: `../bob/avatar.txt` leads outside the restricted directory\n"
    );
    assert_eq!(host.file("uploads/carol/avatar.txt"), Some("carol's"));
    assert_eq!(host.file("uploads/bob/avatar.txt"), Some("bob's"));
}

#[test]
fn each_directory_capability_narrows_and_dot_dot_inside_the_root_resolves() {
    let mut host = FakeHost::new()
        .with_file("srv/app/a/note.txt", "a note")
        .with_file("srv/app/b/note.txt", "b note")
        .with_dir("srv/app/out");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let app = fs.restrict_to("/srv/app")
    io.println(shown(app.restrict_to(Path.new("a/../b")).read_only().read_to_string(Path.new("note.txt"))))?
    let read_b = read.restrict_to("srv/app").restrict_to(Path.new("b"))
    io.println(shown(read_b.read_to_string(Path.new("note.txt"))))?
    let out = write.restrict_to("srv/app").restrict_to(Path.new("./out/"))
    io.println(done(out.write(Path.new("x.txt"), "x")))?
    io.println(shown(read_b.restrict_to(Path.new(".")).read_to_string(Path.new("note.txt"))))?"#,
    );
    assert_eq!(out, "b note\nb note\ndone\nb note\n");
    assert_eq!(host.file("srv/app/out/x.txt"), Some("x"));
}

#[test]
fn a_narrowing_above_the_root_reaches_nothing() {
    let mut host = FakeHost::new()
        .with_file("data/sub/inside.txt", "inside")
        .with_file("secret.txt", "secret");
    let out = printed_with(
        &mut host,
        &["../.."],
        r#"    let sub = read.restrict_to("data").restrict_to(Path.new("sub"))
    let up = sub.restrict_to(Path.from(args.get(1).unwrap_or("")))
    io.println(shown(up.read_to_string(Path.new("secret.txt"))))?
    io.println(shown(sub.restrict_to(Path.new("..")).read_to_string(Path.new("sub/inside.txt"))))?
    io.println(shown(sub.restrict_to(Path.new("/data")).read_to_string(Path.new("sub/inside.txt"))))?"#,
    );
    assert_eq!(
        out,
        "REFUSED: cannot restrict to `../..`: `../..` leads outside the restricted directory\n\
         REFUSED: cannot restrict to `..`: `..` leads outside the restricted directory\n\
         REFUSED: cannot restrict to `/data`: `/data` is absolute; \
         a restricted filesystem takes paths relative to its root\n"
    );
}

// ---- Broken roots ------------------------------------------------------------------------

#[test]
fn a_missing_root_fails_every_operation_naming_the_root() {
    let mut host = FakeHost::new().with_file("docs/intro.md", "# Intro");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let docs = read.restrict_to("missing")
    io.println(shown(docs.read_to_string(Path.new("intro.md"))))?
    io.println(listed(docs.list_dir(Path.new("."))))?
    io.println(shown(docs.restrict_to(Path.new("sub")).read_to_string(Path.new("intro.md"))))?
    let site = write.restrict_to("missing")
    io.println(done(site.write(Path.new("x.txt"), "x")))?
    io.println(done(site.create_dir(Path.new("x"))))?
    let app = fs.restrict_to("missing")
    io.println(shown(app.read_only().read_to_string(Path.new("intro.md"))))?
    io.println(done(app.restrict_to(Path.new("sub")).write_only().write(Path.new("x.txt"), "x")))?"#,
    );
    let why = "REFUSED: cannot restrict to `missing`: no such file or directory: /missing\n";
    assert_eq!(out, why.repeat(7));
    assert_eq!(host.files_under("/").len(), 1, "nothing was written");
}

#[test]
fn a_missing_subdirectory_fails_every_operation_naming_it() {
    let host = FakeHost::new().with_file("docs/intro.md", "# Intro");
    let out = printed(
        host,
        r#"    let gone = read.restrict_to("docs").restrict_to(Path.new("gone"))
    io.println(shown(gone.read_to_string(Path.new("intro.md"))))?
    io.println(listed(gone.list_dir(Path.new("."))))?"#,
    );
    let why = "REFUSED: cannot restrict to `gone`: no such file or directory: /docs/gone\n";
    assert_eq!(out, why.repeat(2));
}

// ---- Granting ----------------------------------------------------------------------------

#[test]
fn main_cannot_be_granted_a_directory_capability() {
    for ty in ["Dir", "DirRead", "DirWrite"] {
        let src = format!("fn main(dir: {ty}) {{\n}}\n");
        assert_refused(&src, "E0370");
        let outcome = run_fake(&src, &[]);
        assert_eq!(
            outcome.diagnostics[0].message,
            format!(
                "`oskac run` cannot grant `{ty}`: only attenuation makes a directory capability, \
                 with `restrict_to` on `Fs`, `FsRead` or `FsWrite`"
            )
        );
    }
}

// ---- Confinement on the real host --------------------------------------------------------

/// A narrowing from untrusted text, on [`RealHost`] against a real
/// temporary directory with real symlinks. A program's results go to a
/// report file in a directory of their own, since the real host prints to
/// the real stdout.
#[cfg(unix)]
mod real {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use oska_run::{run, RealHost};

    use super::SHOWN;

    /// A fresh directory under the system temp dir, removed on drop:
    ///
    /// ```text
    /// root/inside.txt         "inside text"
    /// root/sub/nested.txt     "nested text"
    /// outside/secret.txt      "outside text"
    /// report/                 where a program writes what it saw
    /// ```
    struct Sandbox {
        base: PathBuf,
    }

    impl Sandbox {
        fn new(test: &str) -> Sandbox {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let unique = format!(
                "oska-dircap-{test}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let base = std::env::temp_dir().join(unique);
            for dir in ["root/sub", "outside", "report"] {
                std::fs::create_dir_all(base.join(dir)).expect("create a directory");
            }
            std::fs::write(base.join("root/inside.txt"), "inside text").expect("write inside");
            std::fs::write(base.join("root/sub/nested.txt"), "nested text").expect("write nested");
            std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write outside");
            let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
            Sandbox { base }
        }

        fn path(&self, at: &str) -> PathBuf {
            self.base.join(at)
        }

        /// A symlink at `<base>/<at>` pointing to `target`.
        fn link(&self, at: &str, target: &Path) {
            std::os::unix::fs::symlink(target, self.path(at)).expect("create a symlink");
        }

        /// What `body` saw, run with `root: Dir` restricted to `root`
        /// beneath the sandbox, and `user` the untrusted text `user`.
        /// Each `say(...)` in `body` adds a line.
        fn saw(&self, root: &str, user: &str, body: &str) -> String {
            let src = format!(
                "fn main(fs: Fs, args: Args) -> Result<(), IoError> {{\n\
                 \x20   let root = fs.restrict_to(\"{}\")\n\
                 \x20   let user = args.get(1).unwrap_or(\"\")\n\
                 \x20   var seen: Untrusted<String> = \"\"\n\
                 {body}\n\
                 \x20   fs.write_only().restrict_to(\"{}\").write(Path.new(\"seen.txt\"), seen)\n\
                 }}\n\
                 \n\
                 fn said(seen: Untrusted<String>, line: Untrusted<String>) -> Untrusted<String> {{\n\
                 \x20   \"{{seen}}\\n{{line}}\"\n\
                 }}\n{SHOWN}",
                self.path(root).display(),
                self.path("report").display(),
                body = body.replace("say(", "seen = said(seen, "),
            );
            let mut host = RealHost::new();
            let args = ["prog.osk".to_string(), user.to_string()];
            let outcome = run(&src, &mut host, &args);
            assert!(
                outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
                "{src} should run: {outcome:#?}"
            );
            let seen =
                std::fs::read_to_string(self.path("report/seen.txt")).expect("read the report");
            seen.trim_start_matches('\n').to_string()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// What reading `file` sees beneath `root` narrowed to the untrusted
    /// `user`.
    fn read_beneath(sandbox: &Sandbox, user: &str, file: &str) -> String {
        sandbox.saw(
            "root",
            user,
            &format!(
                "    let narrowed = root.read_only().restrict_to(Path.from(user))\n\
                 \x20   say(shown(narrowed.read_to_string(Path.new(\"{file}\"))))"
            ),
        )
    }

    #[test]
    fn a_narrowing_from_untrusted_text_reads_its_subdirectory() {
        let sandbox = Sandbox::new("narrow-ok");
        assert_eq!(read_beneath(&sandbox, "sub", "nested.txt"), "nested text");
        assert_eq!(
            read_beneath(&sandbox, "./sub/", "nested.txt"),
            "nested text"
        );
    }

    #[test]
    fn a_narrowing_to_a_symlink_at_any_component_is_refused() {
        let sandbox = Sandbox::new("narrow-link");
        sandbox.link("root/out", &sandbox.path("outside"));
        sandbox.link("root/sub/out", &sandbox.path("outside"));
        sandbox.link("root/alias", &sandbox.path("root/sub"));
        sandbox.link("root/gone", &sandbox.path("nothing-here"));
        for user in ["out", "sub/out", "alias", "gone"] {
            let seen = read_beneath(&sandbox, user, "secret.txt");
            assert!(
                seen.starts_with(&format!("REFUSED: cannot restrict to `{user}`: "))
                    && seen.contains("symlink"),
                "narrowing to `{user}` saw {seen:?}"
            );
        }
    }

    #[test]
    fn a_narrowing_above_the_root_or_absolute_is_refused() {
        let sandbox = Sandbox::new("narrow-above");
        let outside = sandbox.path("outside").display().to_string();
        for (user, why) in [
            ("../outside", "leads outside the restricted directory"),
            ("sub/../..", "leads outside the restricted directory"),
            (outside.as_str(), "is absolute"),
        ] {
            let seen = read_beneath(&sandbox, user, "secret.txt");
            assert!(
                seen.starts_with(&format!("REFUSED: cannot restrict to `{user}`: "))
                    && seen.contains(why),
                "narrowing to `{user}` saw {seen:?}"
            );
        }
    }

    #[test]
    fn a_write_through_a_narrowing_to_a_symlink_creates_nothing_outside() {
        let sandbox = Sandbox::new("narrow-write");
        sandbox.link("root/out", &sandbox.path("outside"));
        let seen = sandbox.saw(
            "root",
            "out",
            "    let narrowed = root.write_only().restrict_to(Path.from(user))\n\
             \x20   say(done(narrowed.write(Path.new(\"planted.txt\"), \"pwned\")))\n\
             \x20   say(done(narrowed.create_dir(Path.new(\"planted\"))))",
        );
        let why = "REFUSED: cannot restrict to `out`: ";
        assert!(
            seen.lines().count() == 2 && seen.lines().all(|l| l.starts_with(why)),
            "{seen:?}"
        );
        assert!(!sandbox.path("outside/planted.txt").exists());
        assert!(!sandbox.path("outside/planted").exists());
    }

    #[test]
    fn a_symlinked_root_fails_every_operation_naming_it() {
        let sandbox = Sandbox::new("root-link");
        sandbox.link("root-link", &sandbox.path("root"));
        let seen = sandbox.saw(
            "root-link",
            "sub",
            "    say(shown(root.read_only().read_to_string(Path.new(\"inside.txt\"))))\n\
             \x20   say(listed(root.read_only().list_dir(Path.new(\".\"))))\n\
             \x20   say(done(root.write_only().write(Path.new(\"x.txt\"), \"x\")))\n\
             \x20   say(shown(root.restrict_to(Path.from(user)).read_only().read_to_string(Path.new(\"nested.txt\"))))",
        );
        let root = sandbox.path("root-link").display().to_string();
        let why = format!("REFUSED: cannot restrict to `{root}`: ");
        assert!(
            seen.lines().count() == 4
                && seen
                    .lines()
                    .all(|l| l.starts_with(&why) && l.contains("symlink")),
            "{seen:?}"
        );
        assert!(!sandbox.path("root/x.txt").exists());
    }
}
