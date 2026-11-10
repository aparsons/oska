//! Capabilities and confinement (#64): `main` gets the capabilities it
//! declares, attenuation narrows them for real, and a restricted
//! filesystem never reaches outside its root.
//!
//! The confinement tests run on [`RealHost`] against a real temporary
//! directory, with real symlinks, so they exercise the OS's own path
//! resolution. Everything else runs on [`FakeHost`].

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{assert_panics, assert_refused, assert_runs, run_fake, run_on};
use oska_run::{run, FakeHost, Host, Output, RealHost, RunOutcome};

// ---- A real temporary directory --------------------------------------------

/// A fresh directory under the system temp dir, removed on drop. It holds
/// `root`, the directory a program is confined to, and `outside`, a
/// sibling holding a file it must never read.
struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-run-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("root/sub")).expect("create the root");
        std::fs::create_dir_all(base.join("outside")).expect("create the outside");
        std::fs::write(base.join("root/inside.txt"), "inside text").expect("write inside");
        std::fs::write(base.join("root/sub/nested.txt"), "nested text").expect("write nested");
        std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write outside");
        // Canonical, so paths built from it match what the OS reports.
        let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
        Sandbox { base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn outside(&self) -> PathBuf {
        self.base.join("outside")
    }

    /// A symlink at `root/<name>` pointing to `target`.
    fn link(&self, name: &str, target: &Path) {
        std::os::unix::fs::symlink(target, self.root().join(name)).expect("create a symlink");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// [`RealHost`] with its stdout recorded, so a test can see what a
/// program printed. Every other operation goes straight to the real OS.
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
    fn read_file(&mut self, path: &Path, beneath: Option<&Path>) -> std::io::Result<String> {
        self.real.read_file(path, beneath)
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

/// Run `src` on the real OS, recording stdout.
fn run_real(src: &str) -> RunOutcome {
    let mut host = Recording {
        real: RealHost::new(),
        stdout: String::new(),
    };
    let outcome = run(src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src} should run: {outcome:#?}"
    );
    outcome
}

/// What a program prints when it reads `path` through `fs` restricted to
/// `root`: the file's text, or `REFUSED` when the read fails.
fn read_confined(root: &Path, path: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n\
         \x20   let text = local.read_to_string(Path.new(\"{path}\")).unwrap_or(\"REFUSED\")\n\
         \x20   let _ = io.println(text)\n\
         }}\n",
        root.display()
    );
    run_real(&src).stdout
}

/// Write `contents` to `path` through `fs` restricted to `root`, and
/// return what the program printed: `written` or `REFUSED`.
fn write_confined(root: &Path, path: &str, contents: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) {{\n\
         \x20   let local = fs.restrict_to(\"{}\").write_only()\n\
         \x20   let done = local.write(Path.new(\"{path}\"), \"{contents}\").is_ok()\n\
         \x20   let _ = io.println(if done {{ \"written\" }} else {{ \"REFUSED\" }})\n\
         }}\n",
        root.display()
    );
    run_real(&src).stdout
}

// ---- Confinement: escapes ----------------------------------------------------

#[test]
fn a_symlink_to_a_file_outside_the_root_is_refused() {
    let sandbox = Sandbox::new("symlink-file");
    sandbox.link("escape.txt", &sandbox.outside().join("secret.txt"));
    assert_eq!(read_confined(&sandbox.root(), "escape.txt"), "REFUSED\n");
}

#[test]
fn a_symlink_to_a_directory_outside_the_root_is_refused() {
    let sandbox = Sandbox::new("symlink-dir");
    sandbox.link("out", &sandbox.outside());
    assert_eq!(
        read_confined(&sandbox.root(), "out/secret.txt"),
        "REFUSED\n"
    );
}

#[test]
fn a_dot_dot_traversal_out_of_the_root_is_refused() {
    let sandbox = Sandbox::new("dotdot");
    assert_eq!(
        read_confined(&sandbox.root(), "../outside/secret.txt"),
        "REFUSED\n"
    );
    assert_eq!(
        read_confined(&sandbox.root(), "sub/../../outside/secret.txt"),
        "REFUSED\n"
    );
}

#[test]
fn an_absolute_path_is_refused_even_inside_the_root() {
    let sandbox = Sandbox::new("absolute");
    let outside = sandbox.outside().join("secret.txt");
    assert_eq!(
        read_confined(&sandbox.root(), &outside.display().to_string()),
        "REFUSED\n"
    );
    let inside = sandbox.root().join("inside.txt");
    assert_eq!(
        read_confined(&sandbox.root(), &inside.display().to_string()),
        "REFUSED\n"
    );
}

#[test]
fn an_empty_path_is_refused() {
    let sandbox = Sandbox::new("empty");
    assert_eq!(read_confined(&sandbox.root(), ""), "REFUSED\n");
}

#[test]
fn a_write_through_a_dangling_symlink_out_of_the_root_is_refused() {
    let sandbox = Sandbox::new("write-dangling");
    let target = sandbox.outside().join("planted.txt");
    sandbox.link("plant.txt", &target);
    assert_eq!(
        write_confined(&sandbox.root(), "plant.txt", "pwned"),
        "REFUSED\n"
    );
    assert!(!target.exists(), "nothing is written outside the root");
}

#[test]
fn a_write_out_of_the_root_is_refused() {
    let sandbox = Sandbox::new("write-escape");
    assert_eq!(
        write_confined(&sandbox.root(), "../outside/new.txt", "pwned"),
        "REFUSED\n"
    );
    assert_eq!(
        write_confined(&sandbox.root(), "sub/..", "pwned"),
        "REFUSED\n"
    );
    assert!(!sandbox.outside().join("new.txt").exists());
}

#[test]
fn restricting_a_restricted_filesystem_never_widens_it() {
    let sandbox = Sandbox::new("rerestrict");
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let wider = fs.restrict_to(\"{}\").restrict_to(\"..\")\n\
         \x20   let text = wider.read_to_string(Path.new(\"outside/secret.txt\")).unwrap_or(\"REFUSED\")\n\
         \x20   let _ = io.println(text)\n\
         }}\n",
        sandbox.root().display()
    );
    assert_eq!(run_real(&src).stdout, "REFUSED\n");
}

/// Run `src` on a bare [`RealHost`], no wrapper, and return its exit
/// code: 0 when `main` returned `Ok`, 1 when it returned `Err`.
fn exit_code_on_real_host(src: &str) -> i32 {
    let outcome = run(src, &mut RealHost::new(), &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src} should run: {outcome:#?}"
    );
    outcome.exit_code
}

/// The exit code of a program that writes `contents` to `path` through a
/// filesystem restricted to `root`, on a bare [`RealHost`]: 0 when the
/// write succeeded, 1 when it was refused.
fn write_direct(root: &Path, path: &str, contents: &str) -> i32 {
    exit_code_on_real_host(&format!(
        "fn main(fs: FsWrite) -> Result<(), IoError> {{\n\
         \x20   fs.restrict_to(\"{}\").write(Path.new(\"{path}\"), \"{contents}\")\n\
         }}\n",
        root.display()
    ))
}

/// The exit code of a program that reads `path` through a filesystem
/// restricted to `root`, on a bare [`RealHost`]: 0 when the read
/// succeeded, 1 when it was refused.
fn read_direct(root: &Path, path: &str) -> i32 {
    exit_code_on_real_host(&format!(
        "fn main(fs: FsRead) -> Result<(), IoError> {{\n\
         \x20   let _ = fs.restrict_to(\"{}\").read_to_string(Path.new(\"{path}\"))?\n\
         \x20   Ok(())\n\
         }}\n",
        root.display()
    ))
}

#[test]
fn a_write_through_a_dangling_symlink_with_a_trailing_separator_is_refused() {
    let sandbox = Sandbox::new("write-dangling-slash");
    let target = sandbox.outside().join("pwned1");
    sandbox.link("dang", &target);
    std::os::unix::fs::symlink(&target, sandbox.root().join("sub/dang"))
        .expect("create a nested symlink");
    for path in ["dang/", "dang/.", "sub/dang/", "dang//", "sub/dang/."] {
        assert_eq!(
            write_confined(&sandbox.root(), path, "pwned"),
            "REFUSED\n",
            "writing `{path}`"
        );
        assert_eq!(write_direct(&sandbox.root(), path, "pwned"), 1, "{path}");
        assert!(!target.exists(), "`{path}` wrote outside the root");
    }
}

#[test]
fn a_write_whose_last_component_is_not_a_plain_name_is_refused() {
    let sandbox = Sandbox::new("write-not-a-name");
    for path in ["new/", "sub/", "sub/.", ".", "sub/..", "new.txt/"] {
        assert_eq!(write_direct(&sandbox.root(), path, "x"), 1, "{path}");
    }
    assert!(!sandbox.root().join("new").exists());
    assert!(!sandbox.root().join("new.txt").exists());
}

#[test]
fn a_write_to_a_symlink_inside_the_root_is_refused_even_to_an_inside_file() {
    // The final component is opened without following symlinks, so a
    // write never goes through one, wherever it points.
    let sandbox = Sandbox::new("write-symlink-inside");
    sandbox.link("alias.txt", &sandbox.root().join("sub/nested.txt"));
    sandbox.link("dangling-inside", &sandbox.root().join("sub/later.txt"));
    assert_eq!(write_direct(&sandbox.root(), "dangling-inside", "x"), 1);
    assert!(!sandbox.root().join("sub/later.txt").exists());
}

#[test]
fn real_host_opens_a_restricted_file_without_following_a_symlink() {
    // The host's own guard, behind `confine.rs`: what a symlink swapped
    // in after the path was checked would meet.
    let sandbox = Sandbox::new("host-nofollow");
    let root = sandbox.root();
    let target = sandbox.outside().join("swapped");
    sandbox.link("dang", &target);
    sandbox.link("escape.txt", &sandbox.outside().join("secret.txt"));
    sandbox.link("alias.txt", &root.join("inside.txt"));
    let mut host = RealHost::new();
    assert!(host
        .write_file(&root.join("dang"), "pwned", Some(&root))
        .is_err());
    assert!(!target.exists(), "the dangling symlink was followed");
    assert!(host
        .read_file(&root.join("escape.txt"), Some(&root))
        .is_err());
    assert!(host
        .read_file(&root.join("alias.txt"), Some(&root))
        .is_err());
    assert!(host
        .write_file(&root.join("escape.txt"), "pwned", Some(&root))
        .is_err());
    assert_eq!(
        std::fs::read_to_string(sandbox.outside().join("secret.txt")).expect("untouched"),
        "outside text"
    );
}

#[test]
fn real_host_refuses_a_restricted_file_that_is_not_under_the_root() {
    let sandbox = Sandbox::new("host-beneath");
    let root = sandbox.root();
    let outside = sandbox.outside().join("secret.txt");
    let mut host = RealHost::new();
    assert!(host.read_file(&outside, Some(&root)).is_err());
    assert!(host.write_file(&outside, "pwned", Some(&root)).is_err());
    assert_eq!(
        std::fs::read_to_string(&outside).expect("untouched"),
        "outside text"
    );
    // Unrestricted, the same host reads it, through a symlink too.
    sandbox.link("escape.txt", &outside);
    assert_eq!(
        host.read_file(&root.join("escape.txt"), None)
            .expect("an unrestricted read follows symlinks"),
        "outside text"
    );
    assert_eq!(
        host.read_file(&root.join("inside.txt"), Some(&root))
            .expect("a file under the root"),
        "inside text"
    );
}

#[test]
fn real_host_confines_reads_and_writes_directly() {
    let sandbox = Sandbox::new("direct");
    sandbox.link("escape.txt", &sandbox.outside().join("secret.txt"));
    sandbox.link("out", &sandbox.outside());
    assert_eq!(read_direct(&sandbox.root(), "inside.txt"), 0);
    assert_eq!(read_direct(&sandbox.root(), "sub/../inside.txt"), 0);
    assert_eq!(read_direct(&sandbox.root(), "escape.txt"), 1);
    assert_eq!(read_direct(&sandbox.root(), "out/secret.txt"), 1);
    assert_eq!(read_direct(&sandbox.root(), "../outside/secret.txt"), 1);
    assert_eq!(write_direct(&sandbox.root(), "sub/new.txt", "fresh"), 0);
    assert_eq!(
        std::fs::read_to_string(sandbox.root().join("sub/new.txt")).expect("written"),
        "fresh"
    );
    assert_eq!(write_direct(&sandbox.root(), "inside.txt", "again"), 0);
    assert_eq!(
        std::fs::read_to_string(sandbox.root().join("inside.txt")).expect("rewritten"),
        "again"
    );
    assert_eq!(write_direct(&sandbox.root(), "out/new.txt", "pwned"), 1);
    assert_eq!(write_direct(&sandbox.root(), "escape.txt", "pwned"), 1);
    assert_eq!(
        std::fs::read_to_string(sandbox.outside().join("secret.txt")).expect("untouched"),
        "outside text"
    );
    assert!(!sandbox.outside().join("new.txt").exists());
}

// ---- Confinement: what stays allowed -------------------------------------

#[test]
fn a_file_inside_the_root_is_read() {
    let sandbox = Sandbox::new("inside");
    assert_eq!(
        read_confined(&sandbox.root(), "inside.txt"),
        "inside text\n"
    );
    assert_eq!(
        read_confined(&sandbox.root(), "sub/nested.txt"),
        "nested text\n"
    );
    assert_eq!(
        read_confined(&sandbox.root(), "sub/../inside.txt"),
        "inside text\n"
    );
}

#[test]
fn a_symlink_inside_the_root_to_a_file_inside_the_root_is_read() {
    let sandbox = Sandbox::new("symlink-inside");
    sandbox.link("alias.txt", &sandbox.root().join("sub/nested.txt"));
    assert_eq!(read_confined(&sandbox.root(), "alias.txt"), "nested text\n");
}

#[test]
fn a_restriction_to_a_subdirectory_reads_relative_to_it() {
    let sandbox = Sandbox::new("subroot");
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let sub = fs.restrict_to(\"{}\").restrict_to(\"sub\")\n\
         \x20   let text = sub.read_to_string(Path.new(\"nested.txt\")).unwrap_or(\"REFUSED\")\n\
         \x20   let _ = io.println(text)\n\
         }}\n",
        sandbox.root().display()
    );
    assert_eq!(run_real(&src).stdout, "nested text\n");
}

#[test]
fn a_file_that_does_not_exist_yet_is_written_inside_the_root() {
    let sandbox = Sandbox::new("write-new");
    assert_eq!(
        write_confined(&sandbox.root(), "sub/new.txt", "fresh"),
        "written\n"
    );
    let written = std::fs::read_to_string(sandbox.root().join("sub/new.txt"));
    assert_eq!(written.expect("the file exists"), "fresh");
}

// ---- Arguments, environment, and streams (FakeHost) ------------------------

#[test]
fn args_hold_the_program_path_then_the_arguments() {
    let src = "fn main(io: Stdout, args: Args) {\n\
               \x20   let _ = io.println(args.get(0).unwrap_or(\"none\"))\n\
               \x20   let _ = io.println(args.get(1).unwrap_or(\"none\"))\n\
               \x20   let _ = io.println(args.get(2).unwrap_or(\"none\"))\n\
               }\n";
    assert_runs(src, &["input.txt"], "prog.osk\ninput.txt\nnone\n");
}

#[test]
fn env_reads_a_variable_and_reports_a_missing_one() {
    let src = "fn main(io: Stdout, env: Env) {\n\
               \x20   let _ = io.println(env.get(\"HOME\").unwrap_or(\"unset\"))\n\
               \x20   let _ = io.println(env.get(\"NOPE\").unwrap_or(\"unset\"))\n\
               }\n";
    let outcome = run_on(FakeHost::new().with_env("HOME", "/home/oska"), src, &[]);
    assert_eq!(outcome.stdout, "/home/oska\nunset\n", "{outcome:#?}");
}

#[test]
fn dbg_redacts_a_secret_and_shows_other_values() {
    let src = "fn main(env: Env) {\n\
               \x20   let key = dbg(env.secret(\"API_KEY\"))\n\
               \x20   let _ = dbg(\"plain\")\n\
               \x20   let _ = dbg(42)\n\
               }\n";
    let outcome = run_on(FakeHost::new().with_env("API_KEY", "hunter2"), src, &[]);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(
        outcome.stderr,
        "[dbg] <redacted>\n[dbg] \"plain\"\n[dbg] 42\n"
    );
    assert!(!outcome.stderr.contains("hunter2"));
}

#[test]
fn a_secret_is_exposed_and_compared_only_on_request() {
    let src = "fn main(io: Stdout, env: Env) {\n\
               \x20   let key = env.secret(\"API_KEY\")\n\
               \x20   let same = key.constant_time_eq(\"hunter2\")\n\
               \x20   let other = key.constant_time_eq(\"hunter3\")\n\
               \x20   let _ = io.println(if same && !other { \"true false\" } else { \"wrong\" })\n\
               \x20   let _ = io.println(key.expose())\n\
               }\n";
    let outcome = run_on(FakeHost::new().with_env("API_KEY", "hunter2"), src, &[]);
    assert_eq!(outcome.stdout, "true false\nhunter2\n", "{outcome:#?}");
}

#[test]
fn a_missing_secret_panics_rather_than_running_on_without_it() {
    assert_panics(
        "fn main(env: Env) {\n    let key = env.secret(\"API_KEY\")\n}\n",
        "P0020",
        "env.secret(\"API_KEY\")",
    );
}

#[test]
fn stderr_and_stdin_reach_the_standard_streams() {
    let src = "fn main(input: Stdin, err: Stderr, args: Args) {\n\
               \x20   // `args.get(9)` is a `None` to fall back on.\n\
               \x20   let first = input.read_line().unwrap_or(args.get(9)).unwrap_or(\"eof\")\n\
               \x20   let second = input.read_line().unwrap_or(args.get(9)).unwrap_or(\"eof\")\n\
               \x20   let _ = err.println(first)\n\
               \x20   let _ = err.println(second)\n\
               }\n";
    let outcome = run_on(FakeHost::new().with_stdin("only line\n"), src, &[]);
    assert_eq!(outcome.stderr, "only line\neof\n", "{outcome:#?}");
}

// ---- Clock and randomness (FakeHost) ---------------------------------------

#[test]
fn the_fake_clock_stands_at_the_unix_epoch() {
    assert_runs(
        "fn main(io: Stdout, clock: Clock) {\n    let now = clock.now_millis()\n    let _ = io.println(\"{now}\")\n}\n",
        &[],
        "0\n",
    );
}

#[test]
fn random_numbers_vary_but_repeat_for_the_same_seed() {
    let src = "fn main(io: Stdout, random: Random) {\n\
               \x20   let a = random.next_int()\n\
               \x20   let b = random.next_int()\n\
               \x20   let _ = io.println(\"{a} {b}\")\n\
               }\n";
    let first = run_fake(src, &[]).stdout;
    assert_eq!(first, run_fake(src, &[]).stdout, "a fixed seed repeats");
    let numbers: Vec<i128> = first
        .split_whitespace()
        .map(|n| n.parse().expect("an integer"))
        .collect();
    assert_eq!(numbers.len(), 2, "{first:?}");
    assert_ne!(numbers[0], numbers[1]);
    assert!(numbers.iter().all(|n| (0..1 << 63).contains(n)));
}

// ---- Files (FakeHost) ------------------------------------------------------

#[test]
fn a_file_written_through_fs_is_read_back() {
    let src = "fn main(io: Stdout, fs: Fs) {\n\
               \x20   let data = fs.restrict_to(\"/data\")\n\
               \x20   let _ = data.write_only().write(Path.new(\"out.txt\"), \"saved\")\n\
               \x20   let text = data.read_only().read_to_string(Path.new(\"out.txt\"))\n\
               \x20   let _ = io.println(text.unwrap_or(\"REFUSED\"))\n\
               }\n";
    let outcome = run_on(FakeHost::new().with_file("data/keep.txt", ""), src, &[]);
    assert_eq!(outcome.stdout, "saved\n", "{outcome:#?}");
}

#[test]
fn a_read_only_filesystem_cannot_write_and_a_write_only_one_cannot_read() {
    // The checker refuses both before the program runs: `FsRead` has no
    // `write`, and `FsWrite` no `read_to_string`.
    assert_refused(
        "fn main(fs: Fs) {\n    let _ = fs.read_only().write(Path.new(\"a\"), \"x\")\n}\n",
        "E0228",
    );
    assert_refused(
        "fn main(fs: Fs) {\n    let _ = fs.write_only().read_to_string(Path.new(\"a\"))\n}\n",
        "E0228",
    );
}

// ---- Capabilities with no runtime yet --------------------------------------

#[test]
fn a_program_given_net_is_refused_before_it_starts() {
    assert_refused("fn main(net: Net) {\n}\n", "E0370");
    assert_refused(
        "fn main(io: Stdout, net: NetConnect) {\n    let _ = io.println(\"hi\")\n}\n",
        "E0370",
    );
}

#[test]
fn a_program_importing_proc_or_sql_is_refused_before_it_starts() {
    for module in ["std.proc", "std.sql"] {
        assert_refused(
            &format!("use {module}\nfn main(io: Stdout) {{\n    let _ = io.println(\"hi\")\n}}\n"),
            "E0371",
        );
    }
}
