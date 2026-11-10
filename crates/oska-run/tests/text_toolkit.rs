//! The text toolkit at runtime (#72): `starts_with`, `ends_with`,
//! `replace`, `join`, `chars`, `slice`, and `repeat` on `String`, and
//! `FsRead.list_dir`.
//!
//! The string tests run on non-ASCII text, where a byte offset and a
//! character index differ. The `list_dir` confinement tests run on
//! [`RealHost`] against a real temporary directory, with real symlinks.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{assert_panics, assert_runs, run_fake, run_on};
use oska_run::{run, FakeHost, Host, Output, RealHost, RootHandle, RunOutcome};

/// A program whose `main` runs `body` with `io: Stdout` in scope.
fn program(body: &str) -> String {
    format!("fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// What `expr`, a `String`, prints.
fn assert_prints(expr: &str, expected: &str) {
    let body = format!("    io.println({expr})");
    assert_runs(&program(&body), &[], &format!("{expected}\n"));
}

/// What `expr`, a `Bool`, prints.
fn assert_bool(expr: &str, expected: bool) {
    let body = format!("    io.println(if {expr} {{ \"yes\" }} else {{ \"no\" }})");
    let expected = if expected { "yes\n" } else { "no\n" };
    assert_runs(&program(&body), &[], expected);
}

// ---- starts_with and ends_with ---------------------------------------------

#[test]
fn starts_with_matches_a_prefix() {
    assert_bool("\"héllo wörld\".starts_with(\"hé\")", true);
    assert_bool("\"héllo wörld\".starts_with(\"he\")", false);
    assert_bool("\"héllo\".starts_with(\"\")", true);
    assert_bool("\"hé\".starts_with(\"héllo\")", false);
}

#[test]
fn ends_with_matches_a_suffix() {
    assert_bool("\"héllo wörld\".ends_with(\"wörld\")", true);
    assert_bool("\"héllo wörld\".ends_with(\"world\")", false);
    assert_bool("\"日本語\".ends_with(\"本語\")", true);
    assert_bool("\"héllo\".ends_with(\"\")", true);
}

// ---- replace -----------------------------------------------------------

#[test]
fn replace_replaces_every_occurrence() {
    assert_prints("\"a-b-c\".replace(\"-\", \"+\")", "a+b+c");
    assert_prints("\"aaa\".replace(\"aa\", \"b\")", "ba");
    assert_prints("\"none\".replace(\"x\", \"y\")", "none");
    assert_prints("\"gone\".replace(\"gone\", \"\")", "");
    // An empty pattern matches nowhere.
    assert_prints("\"abc\".replace(\"\", \"-\")", "abc");
}

#[test]
fn replace_handles_non_ascii_text() {
    assert_prints("\"naïve café\".replace(\"é\", \"e\")", "naïve cafe");
    assert_prints("\"日本語\".replace(\"本\", \"🎉\")", "日🎉語");
}

#[test]
fn replace_escapes_html_the_way_the_site_will() {
    assert_prints(
        "\"<a href=\\\"x\\\">&</a>\".replace(\"&\", \"&amp;\").replace(\"<\", \"&lt;\")",
        "&lt;a href=\"x\">&amp;&lt;/a>",
    );
}

// ---- join --------------------------------------------------------------

#[test]
fn join_puts_the_separator_between_parts() {
    assert_prints("\", \".join(\"a,b,c\".split(\",\"))", "a, b, c");
    assert_prints("\"\".join(\"a,b,c\".split(\",\"))", "abc");
    assert_prints("\"-\".join(\"solo\".split(\",\"))", "solo");
}

#[test]
fn join_of_an_empty_list_is_empty() {
    assert_prints("\"-\".join(List.new())", "");
}

#[test]
fn join_handles_non_ascii_text() {
    assert_prints(
        "\" · \".join(\"日本,語,🎉\".split(\",\"))",
        "日本 · 語 · 🎉",
    );
}

// ---- chars -------------------------------------------------------------

#[test]
fn chars_is_one_text_per_character() {
    let body = "    var out = \"\"\n    for c in \"héllo\".chars() {\n        out = \"{out}[{c}]\"\n    }\n    io.println(out)";
    assert_runs(&program(body), &[], "[h][é][l][l][o]\n");
}

#[test]
fn chars_counts_characters_not_bytes() {
    let body = "    let text = \"日本語🎉\"\n    let chars = text.chars().len()\n    let bytes = text.byte_len()\n    io.println(\"{chars} {bytes}\")";
    assert_runs(&program(body), &[], "4 13\n");
}

#[test]
fn chars_of_the_empty_text_is_empty() {
    let body = "    let none = \"\".chars().len()\n    io.println(\"{none}\")";
    assert_runs(&program(body), &[], "0\n");
}

// ---- slice -------------------------------------------------------------

#[test]
fn slice_takes_characters_from_start_up_to_end() {
    assert_prints("\"hello\".slice(1, 4)", "ell");
    assert_prints("\"hello\".slice(0, 5)", "hello");
    assert_prints("\"hello\".slice(2, 2)", "");
    assert_prints("\"hello\".slice(5, 5)", "");
}

#[test]
fn slice_indexes_characters_not_bytes() {
    assert_prints("\"héllo wörld\".slice(1, 4)", "éll");
    assert_prints("\"日本語🎉\".slice(2, 4)", "語🎉");
    assert_prints("\"日本語🎉\".slice(0, 1)", "日");
}

#[test]
fn slice_past_the_end_panics() {
    let src = program("    io.println(\"héllo\".slice(2, 6))");
    assert_panics(&src, "P0035", "\"héllo\".slice(2, 6)");
}

#[test]
fn slice_with_a_negative_start_panics() {
    let src = program("    io.println(\"hello\".slice(-1, 2))");
    assert_panics(&src, "P0035", "\"hello\".slice(-1, 2)");
}

#[test]
fn slice_with_start_past_end_panics() {
    let src = program("    io.println(\"hello\".slice(3, 2))");
    assert_panics(&src, "P0035", "\"hello\".slice(3, 2)");
}

// ---- repeat ------------------------------------------------------------

#[test]
fn repeat_repeats_the_text() {
    assert_prints("\"ab\".repeat(3)", "ababab");
    assert_prints("\"ab\".repeat(1)", "ab");
    assert_prints("\"ab\".repeat(0)", "");
    assert_prints("\"\".repeat(5)", "");
}

#[test]
fn repeat_handles_non_ascii_text() {
    assert_prints("\"é日\".repeat(3)", "é日é日é日");
}

#[test]
fn repeat_with_a_negative_count_panics() {
    let src = program("    io.println(\"ab\".repeat(-1))");
    assert_panics(&src, "P0036", "\"ab\".repeat(-1)");
}

#[test]
fn repeat_with_a_result_too_long_to_build_panics() {
    let src = program("    io.println(\"ab\".repeat(9223372036854775807))");
    assert_panics(&src, "P0037", "\"ab\".repeat(9223372036854775807)");
    let src = program("    io.println(\"\".repeat(9223372036854775807))");
    assert_runs(&src, &[], "\n");
}

// ---- list_dir on the fake host ----------------------------------------------

fn fake_with_files() -> FakeHost {
    FakeHost::new()
        .with_file("docs/b.md", "b")
        .with_file("docs/a.md", "a")
        .with_file("docs/é.md", "é")
        .with_file("docs/guide/install.md", "i")
        .with_file("docs/guide/quickstart.md", "q")
        .with_file("other/secret.md", "s")
}

const LIST_DOCS: &str = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let docs = fs.restrict_to(\"docs\")
    for name in docs.list_dir(Path.new(\".\"))? {
        io.println(name)?
    }
    Ok(())
}
";

#[test]
fn list_dir_names_the_entries_sorted() {
    let outcome = run_on(fake_with_files(), LIST_DOCS, &[]);
    assert!(outcome.diagnostics.is_empty(), "{outcome:#?}");
    assert_eq!(outcome.stdout, "a.md\nb.md\nguide\né.md\n");
    assert_eq!(outcome.exit_code, 0);
}

#[test]
fn list_dir_names_an_empty_directory_and_lists_it_as_empty() {
    let host = fake_with_files().with_dir("docs/empty");
    let outcome = run_on(host, LIST_DOCS, &[]);
    assert_eq!(outcome.stdout, "a.md\nb.md\nempty\nguide\né.md\n");
    let src = LIST_DOCS.replace("Path.new(\".\")", "Path.new(\"empty\")");
    let outcome = run_on(fake_with_files().with_dir("docs/empty"), &src, &[]);
    assert_eq!((outcome.exit_code, outcome.stdout.as_str()), (0, ""));
}

#[test]
fn list_dir_lists_a_subdirectory_with_bare_names() {
    let src = LIST_DOCS.replace("Path.new(\".\")", "Path.new(\"guide\")");
    let outcome = run_on(fake_with_files(), &src, &[]);
    assert_eq!(outcome.stdout, "install.md\nquickstart.md\n");
}

#[test]
fn list_dir_of_a_missing_directory_is_an_error() {
    let src = LIST_DOCS.replace("Path.new(\".\")", "Path.new(\"nope\")");
    let outcome = run_on(fake_with_files(), &src, &[]);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
    assert_eq!(outcome.stdout, "");
}

#[test]
fn list_dir_of_a_file_is_an_error() {
    let src = LIST_DOCS.replace("Path.new(\".\")", "Path.new(\"a.md\")");
    let outcome = run_on(fake_with_files(), &src, &[]);
    assert_eq!(outcome.exit_code, 1, "{outcome:#?}");
}

#[test]
fn list_dir_refuses_a_path_out_of_its_root() {
    for path in ["..", "../other", "guide/../../other", "/other", ""] {
        let src = LIST_DOCS.replace("Path.new(\".\")", &format!("Path.new(\"{path}\")"));
        let outcome = run_on(fake_with_files(), &src, &[]);
        assert_eq!(outcome.exit_code, 1, "listing `{path}`: {outcome:#?}");
        assert_eq!(outcome.stdout, "", "listing `{path}`");
    }
}

#[test]
fn list_dir_lists_the_whole_filesystem_through_a_root_of_slash() {
    let src = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    for name in fs.restrict_to(\"/\").list_dir(Path.new(\"other\"))? {
        io.println(name)?
    }
    Ok(())
}
";
    let outcome = run_on(fake_with_files(), src, &[]);
    assert_eq!(outcome.stdout, "secret.md\n");
}

#[test]
fn a_listed_name_can_be_used_to_read_the_file() {
    let src = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let docs = fs.restrict_to(\"docs\")
    for name in docs.list_dir(Path.new(\".\"))? {
        if name.ends_with(\".md\") {
            let text = docs.read_to_string(Path.from(name))?
            io.println(text)?
        }
    }
    Ok(())
}
";
    let outcome = run_on(fake_with_files(), src, &[]);
    assert_eq!(outcome.stdout, "a\nb\né\n");
}

#[test]
fn a_write_only_directory_cannot_list_at_runtime_through_fake() {
    // The checker gives `DirWrite` no `list_dir` (E0228).
    let src = "fn main(fs: FsWrite) {\n    \
               let _ = fs.restrict_to(\"/\").list_dir(Path.new(\".\"))\n}\n";
    let outcome = run_fake(src, &[]);
    assert!(outcome.diagnostics.iter().any(|d| d.code == "E0228"));
}

// ---- list_dir confinement on the real host -------------------------------------

/// A fresh directory under the system temp dir, removed on drop. It holds
/// `root`, the directory a program is confined to, and `outside`, a
/// sibling it must never list.
struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-run-list-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("root/sub")).expect("create the root");
        std::fs::create_dir_all(base.join("outside")).expect("create the outside");
        std::fs::write(base.join("root/zebra.txt"), "z").expect("write zebra");
        std::fs::write(base.join("root/Apple.txt"), "a").expect("write apple");
        std::fs::write(base.join("root/é.txt"), "é").expect("write non-ascii");
        std::fs::write(base.join("root/sub/nested.txt"), "n").expect("write nested");
        std::fs::write(base.join("outside/secret.txt"), "s").expect("write outside");
        let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
        Sandbox { base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn outside(&self) -> PathBuf {
        self.base.join("outside")
    }

    #[cfg(unix)]
    fn link(&self, name: &str, target: &Path) {
        std::os::unix::fs::symlink(target, self.root().join(name)).expect("create a symlink");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// [`RealHost`] with its stdout recorded.
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
        self.stdout.lines().map(str::to_string).collect()
    }
    fn take_output(&mut self) -> Output {
        Output {
            stdout: std::mem::take(&mut self.stdout),
            stderr: String::new(),
        }
    }
}

/// What a program prints when it lists `path` through a filesystem
/// restricted to `root`: one name per line, or `REFUSED`.
fn list_confined(root: &Path, path: &str) -> String {
    let path = common::literal(path);
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n\
         \x20   match local.list_dir(Path.new(\"{path}\")) {{\n\
         \x20       Ok(names) => {{\n\
         \x20           for name in names {{\n\
         \x20               let _ = io.println(name)\n\
         \x20           }}\n\
         \x20       }}\n\
         \x20       Err(_) => {{\n\
         \x20           let _ = io.println(\"REFUSED\")\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n",
        common::literal(root)
    );
    let mut host = Recording {
        real: RealHost::new(),
        stdout: String::new(),
    };
    let outcome: RunOutcome = run(&src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

#[test]
fn real_host_lists_the_root_sorted() {
    let sandbox = Sandbox::new("sorted");
    assert_eq!(
        list_confined(&sandbox.root(), "."),
        "Apple.txt\nsub\nzebra.txt\né.txt\n"
    );
}

#[test]
fn real_host_lists_a_subdirectory() {
    let sandbox = Sandbox::new("sub");
    assert_eq!(list_confined(&sandbox.root(), "sub"), "nested.txt\n");
    assert_eq!(list_confined(&sandbox.root(), "sub/."), "nested.txt\n");
    assert_eq!(list_confined(&sandbox.root(), "sub/../sub"), "nested.txt\n");
}

#[test]
fn real_host_refuses_to_list_out_of_the_root() {
    let sandbox = Sandbox::new("dotdot");
    for path in ["..", "../outside", "sub/../../outside", "sub/../.."] {
        assert_eq!(
            list_confined(&sandbox.root(), path),
            "REFUSED\n",
            "listing `{path}`"
        );
    }
}

#[test]
#[cfg(unix)]
fn real_host_refuses_a_symlink_to_a_directory_outside_the_root() {
    let sandbox = Sandbox::new("symlink-dir");
    sandbox.link("out", &sandbox.outside());
    assert_eq!(list_confined(&sandbox.root(), "out"), "REFUSED\n");
    assert_eq!(list_confined(&sandbox.root(), "out/."), "REFUSED\n");
}

#[test]
fn real_host_refuses_an_absolute_or_empty_path() {
    let sandbox = Sandbox::new("absolute");
    let outside = sandbox.outside().display().to_string();
    let inside = sandbox.root().display().to_string();
    for path in [outside.as_str(), inside.as_str(), ""] {
        assert_eq!(
            list_confined(&sandbox.root(), path),
            "REFUSED\n",
            "`{path}`"
        );
    }
}

#[test]
fn real_host_refuses_to_list_a_file_or_a_missing_directory() {
    let sandbox = Sandbox::new("not-a-dir");
    assert_eq!(list_confined(&sandbox.root(), "zebra.txt"), "REFUSED\n");
    assert_eq!(list_confined(&sandbox.root(), "missing"), "REFUSED\n");
}

#[test]
#[cfg(unix)]
fn real_host_refuses_a_symlink_to_a_directory_inside_the_root() {
    // Every symlink beneath a root is refused, wherever it points (#95).
    let sandbox = Sandbox::new("symlink-inside");
    sandbox.link("alias", &sandbox.root().join("sub"));
    assert_eq!(list_confined(&sandbox.root(), "alias"), "REFUSED\n");
}

#[test]
#[cfg(unix)]
fn a_symlink_entry_is_listed_by_name_without_following_it() {
    let sandbox = Sandbox::new("symlink-entry");
    sandbox.link("out", &sandbox.outside());
    assert_eq!(
        list_confined(&sandbox.root(), "."),
        "Apple.txt\nout\nsub\nzebra.txt\né.txt\n"
    );
}
