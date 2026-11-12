//! The native half of every runtime test (spec #125): on aarch64 Linux,
//! each program a helper in this module runs on the interpreter is also
//! compiled by the native backend (`oska_native::build`, or
//! `build_tests` for `test` blocks) and run as a real process, and the
//! two must agree on stdout, stderr (a panic's report included), the exit
//! code, and the filesystem afterwards. The interpreter is the oracle: a
//! difference fails the test, as a bug in the native backend.
//!
//! The [`FakeHost`](oska_run::FakeHost) a test set up is reproduced on
//! the real OS: its stdin, its environment (and nothing else), its
//! arguments, and, for a program given the filesystem (`Fs`, `FsRead`,
//! `FsWrite`) or an `#[integration]` test, its files, directories and
//! symlinks, in a fresh directory that the process sees as `/`
//! (`unshare --user --map-current-user --root`, an unprivileged user
//! namespace, so the program's paths are the fake host's and it can reach
//! nothing else). Where user namespaces are not available, the world is
//! the process's working directory instead, for a program that names no
//! absolute path and no `..` and meets no symlink, and any other program
//! given the filesystem is not compared there (a note says so). A plain
//! test's world is the test host in `std.rt`, which fakes what the
//! interpreter's does.
//!
//! What cannot be reproduced is in the one skip list,
//! [`super::skips::SKIPS`], by test, each with its reason.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use oska_check::program::{Program, Type};
use oska_run::{FakeWorld, RunOutcome, TestOptions, TestReport, TestStatus};
use oska_syntax::Diagnostic;

/// The name the executable is run by, as `args[0]`: the interpreter's
/// program path.
const PROGRAM: &str = super::PROGRAM_PATH;

/// How long one native process may run before the comparison gives up on
/// it: far longer than any test program takes.
const TIMEOUT: Duration = Duration::from_secs(120);

/// Compare a run of `src`, which on the interpreter started in `before`
/// with `args` (the program path first) and ended as `outcome` leaving
/// `after`, with the same run as a native executable.
pub fn compare_run(
    src: &str,
    before: &FakeWorld,
    args: &[String],
    outcome: &RunOutcome,
    after: &FakeWorld,
) {
    // A refused program never ran; the checker is the same for both.
    if !outcome.diagnostics.is_empty() || skipped() {
        return;
    }
    compare_program(&checked(src), src, before, args, outcome, after);
}

/// [`compare_run`] for the checked `program`, shown as `src` when the
/// backends differ.
pub fn compare_program(
    program: &Program,
    src: &str,
    before: &FakeWorld,
    args: &[String],
    outcome: &RunOutcome,
    after: &FakeWorld,
) {
    if !outcome.diagnostics.is_empty() || skipped() {
        return;
    }
    let bytes = oska_native::build(program, &|d| d).unwrap_or_else(|refusal| {
        fail(src, &format!("the native backend refused it: {refusal:#?}"))
    });
    let mut texts: Vec<&str> = args.iter().map(String::as_str).collect();
    texts.push(src);
    let Some(sandbox) = Sandbox::new(before, needs_filesystem(program), &texts) else {
        return;
    };
    let native = sandbox.run(&bytes, &args[1..], &stdin_text(before));
    let mut stderr = outcome.stderr.clone();
    if let Some(panic) = &outcome.panic {
        stderr.push_str(&report(panic));
    }
    let interpreted = Run {
        stdout: outcome.stdout.clone(),
        stderr,
        exit_code: Some(outcome.exit_code),
        tree: sandbox.has_world().then(|| expected_tree(after)),
    };
    if native != interpreted {
        fail(
            src,
            &format!(
                "the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}"
            ),
        );
    }
}

/// Compare the `test` blocks of `src`, which the interpreter ran with
/// `options` (its `#[integration]` tests on a fake host holding `before`)
/// and reported as `report`, with the same tests run natively.
pub fn compare_tests(src: &str, before: &FakeWorld, options: TestOptions, report: &TestReport) {
    if !report.diagnostics.is_empty() || skipped() {
        return;
    }
    compare_program_tests(&checked(src), src, before, options, report);
}

/// [`compare_tests`] for the checked `program`, shown as `src` when the
/// backends differ.
pub fn compare_program_tests(
    program: &Program,
    src: &str,
    before: &FakeWorld,
    options: TestOptions,
    report: &TestReport,
) {
    if !report.diagnostics.is_empty() || skipped() {
        return;
    }
    let bytes = oska_native::build_tests(program).unwrap_or_else(|refusal| {
        fail(src, &format!("the native backend refused it: {refusal:#?}"))
    });
    let integration = options.integration && program.tests.iter().any(|t| t.integration);
    let Some(sandbox) = Sandbox::new(before, integration, &[src]) else {
        return;
    };
    let native: Vec<(TestStatus, Option<Diagnostic>)> = program
        .tests
        .iter()
        .enumerate()
        .map(|(index, test)| {
            if test.integration && !options.integration {
                return (TestStatus::Skipped, None);
            }
            let run = sandbox.run(&bytes, &[index.to_string()], "");
            match run.exit_code {
                Some(0) if run.stdout.is_empty() => (TestStatus::Passed, None),
                Some(101) => match oska_native::test_failure(run.stdout.as_bytes()) {
                    Some(failure) => (TestStatus::Failed, Some(failure)),
                    None => fail(src, &format!("test {index} reported no panic: {run:#?}")),
                },
                _ => fail(src, &format!("test {index} ended abnormally: {run:#?}")),
            }
        })
        .collect();
    let interpreted: Vec<(TestStatus, Option<Diagnostic>)> = report
        .tests
        .iter()
        .map(|t| (t.status, t.failure.clone()))
        .collect();
    if native != interpreted {
        fail(
            src,
            &format!(
                "the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}"
            ),
        );
    }
}

/// Whether the running test is in the skip list.
fn skipped() -> bool {
    let case = case();
    super::skips::SKIPS.iter().any(|(name, _)| *name == case)
}

/// The test running now, as the skip list names it: the test file's
/// crate, `::`, and the test's path in it.
fn case() -> String {
    let thread = std::thread::current();
    format!(
        "{}::{}",
        env!("CARGO_CRATE_NAME"),
        thread.name().unwrap_or("?")
    )
}

fn fail(src: &str, what: &str) -> ! {
    panic!(
        "native differential test `{}`: {what}\n--- program\n{src}\n\
         (a native backend bug; if the case cannot run on a real OS, add it to \
         tests/common/skips.rs with the reason)",
        case()
    )
}

fn checked(src: &str) -> Program {
    let report = oska_check::check_module(src);
    match report.program {
        Some(program) if report.diagnostics.is_empty() => program,
        _ => fail(
            src,
            &format!("it does not check: {:#?}", report.diagnostics),
        ),
    }
}

/// Whether `main` is given the filesystem, so the program's world must be
/// set up as the fake host's.
fn needs_filesystem(program: &Program) -> bool {
    program.functions.get("main").is_some_and(|main| {
        main.params.iter().any(|p| match &p.ty {
            Type::Named { name, .. } => matches!(name.as_str(), "Fs" | "FsRead" | "FsWrite"),
            _ => false,
        })
    })
}

/// A panic's report, as a native executable built with no locator
/// writes it to stderr.
fn report(panic: &Diagnostic) -> String {
    format!(
        "error[{}]: {} (bytes {}..{})\n",
        panic.code, panic.message, panic.span.start, panic.span.end
    )
}

/// What the fake host's stdin held, as the text a real stdin gives.
fn stdin_text(world: &FakeWorld) -> String {
    world.stdin.iter().map(|line| format!("{line}\n")).collect()
}

/// What one native process did.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    stdout: String,
    stderr: String,
    /// `None` when a signal killed it.
    exit_code: Option<i32>,
    /// The filesystem afterwards, when the process had one of its own.
    tree: Option<BTreeMap<String, Entry>>,
}

/// One entry of a filesystem tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    File(String),
    Dir,
    Link(String),
    /// Anything else, or a file that is not UTF-8.
    Other,
}

/// The tree a fake host holding `world` has: its files, its symlinks, and
/// every directory, by path relative to `/`.
fn expected_tree(world: &FakeWorld) -> BTreeMap<String, Entry> {
    let name = |path: &Path| -> String {
        path.strip_prefix("/")
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut tree = BTreeMap::new();
    let dir = |path: &Path, tree: &mut BTreeMap<String, Entry>| {
        for ancestor in path.ancestors().filter(|a| *a != Path::new("/")) {
            tree.insert(name(ancestor), Entry::Dir);
        }
    };
    for path in &world.dirs {
        dir(path, &mut tree);
    }
    for path in world.files.keys().chain(world.links.keys()) {
        if let Some(parent) = path.parent() {
            dir(parent, &mut tree);
        }
    }
    for (path, text) in &world.files {
        tree.insert(name(path), Entry::File(text.clone()));
    }
    for (path, target) in &world.links {
        tree.insert(name(path), Entry::Link(target.display().to_string()));
    }
    tree.remove("");
    tree
}

/// How a native process gets the fake host's filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// It is not given one: it runs in an empty directory.
    Bare,
    /// The world is in a directory the process sees as `/`.
    Chroot,
    /// The world is in its working directory, which is as good as `/`
    /// for a program that names no absolute path and no `..` and meets no
    /// symlink: where unprivileged user namespaces are not available.
    Plain,
}

/// A fresh directory a native process runs in, removed on drop: its root
/// holds the fake host's world unless the process is given none.
struct Sandbox {
    base: PathBuf,
    env: BTreeMap<String, String>,
    mode: Mode,
}

impl Sandbox {
    /// The sandbox for a process that runs `texts` (its source and
    /// arguments) in `world`, which it reaches when `needs_world`. `None`
    /// when this machine cannot reproduce the world faithfully: no user
    /// namespaces, and the program names an absolute path, a `..`, or the
    /// world holds a symlink.
    fn new(world: &FakeWorld, needs_world: bool, texts: &[&str]) -> Option<Sandbox> {
        let mode = match needs_world {
            false => Mode::Bare,
            true if chroot_works() => Mode::Chroot,
            true if world.links.is_empty() && texts.iter().all(|t| relative_only(t)) => Mode::Plain,
            true => return None,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "native-{}-{}-{}",
            env!("CARGO_CRATE_NAME"),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("root");
        std::fs::create_dir_all(&root).expect("should create the sandbox");
        if mode != Mode::Bare {
            for dir in &world.dirs {
                std::fs::create_dir_all(inside(&root, dir)).expect("should create a directory");
            }
            for (path, text) in &world.files {
                let file = inside(&root, path);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).expect("should create a directory");
                }
                std::fs::write(&file, text).expect("should write a file");
            }
            for (path, target) in &world.links {
                let link = inside(&root, path);
                if let Some(parent) = link.parent() {
                    std::fs::create_dir_all(parent).expect("should create a directory");
                }
                std::os::unix::fs::symlink(target, &link).expect("should make a symlink");
            }
        }
        Some(Sandbox {
            base,
            env: world.env.clone(),
            mode,
        })
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    /// Whether the process has the world, so the tree afterwards counts.
    fn has_world(&self) -> bool {
        self.mode != Mode::Bare
    }

    /// The executable's path: in the root when the process is chrooted,
    /// as it must be to start there, and beside it otherwise.
    fn exe(&self) -> PathBuf {
        match self.mode {
            Mode::Chroot => self.root().join(PROGRAM),
            Mode::Bare | Mode::Plain => self.base.join(PROGRAM),
        }
    }

    /// Run the executable `bytes` with `args` and `stdin`.
    fn run(&self, bytes: &[u8], args: &[String], stdin: &str) -> Run {
        let exe = self.exe();
        if !exe.exists() {
            write_executable(&exe, bytes);
        }
        let mut command = if self.mode == Mode::Chroot {
            let mut command = Command::new(unshare().expect("chroot_works found it"));
            command
                .args(["--user", "--map-current-user", "--wd=/"])
                .arg(format!("--root={}", self.root().display()))
                .arg(PROGRAM);
            command
        } else {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new(&exe);
            command.arg0(PROGRAM).current_dir(self.root());
            command
        };
        command.args(args).env_clear().envs(&self.env);
        if self.mode == Mode::Chroot && !self.env.contains_key("PATH") {
            // `unshare` finds the program, by its bare name, on `PATH`.
            command.env("PATH", "/");
        }
        let (stdout, stderr, exit_code) = spawn(command, stdin);
        let tree = self.has_world().then(|| {
            let mut tree = BTreeMap::new();
            read_tree(&self.root(), "", &mut tree);
            tree.remove(PROGRAM);
            tree
        });
        Run {
            stdout,
            stderr,
            exit_code,
            tree,
        }
    }
}

/// Whether `text` names no absolute path and no parent directory, as
/// far as a look at its characters can tell: no `"/`, no `..` beside a
/// quote or a slash, and, for an argument, no leading `/`.
fn relative_only(text: &str) -> bool {
    !(text.starts_with('/')
        || ["\"/", "\"..", "../", "/.."]
            .iter()
            .any(|p| text.contains(p))
        || text == "..")
}

/// Whether this machine can run a process in a directory of its own as
/// `/` without privileges: `unshare` is there and unprivileged user
/// namespaces may change root (Ubuntu 24.04's AppArmor forbids it by
/// default). Probed once: changing root into an empty directory and
/// failing to find the program there means it worked. Setting
/// `OSKA_NATIVE_NO_CHROOT` makes it answer no, to try the fallback.
fn chroot_works() -> bool {
    use std::sync::OnceLock;
    static WORKS: OnceLock<bool> = OnceLock::new();
    *WORKS.get_or_init(|| {
        let unshare = match unshare() {
            Some(unshare) if std::env::var_os("OSKA_NATIVE_NO_CHROOT").is_none() => unshare,
            _ => return false,
        };
        let empty = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "native-probe-{}-{}",
            env!("CARGO_CRATE_NAME"),
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&empty);
        let probe = Command::new(unshare)
            .args(["--user", "--map-current-user"])
            .arg(format!("--root={}", empty.display()))
            .arg("/nothing-here")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&empty);
        // `unshare` exits with 127 when the command is not found.
        let works = probe.is_ok_and(|status| status.code() == Some(127));
        if !works {
            eprintln!(
                "note: no unprivileged user namespaces here; native differential runs of \
                 programs that name absolute paths are not compared"
            );
        }
        works
    })
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The fake host's absolute `path` beneath the sandbox's `root`.
fn inside(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

/// Write the executable `bytes` to `path`. Another test thread that
/// forks while this one has a file open for writing holds it open until
/// it execs, and meanwhile the kernel refuses to run the file
/// (`ETXTBSY`). So the bytes go to a scratch file, never run, and `cp`,
/// a process of its own, writes the executable.
fn write_executable(path: &Path, bytes: &[u8]) {
    let scratch = path.with_extension("bytes");
    std::fs::write(&scratch, bytes).expect("should write the executable's bytes");
    let copied = Command::new("cp")
        .arg(&scratch)
        .arg(path)
        .status()
        .expect("cp should run");
    assert!(copied.success(), "cp should copy the executable");
    let _ = std::fs::remove_file(&scratch);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("should make it executable");
}

/// The `unshare` program (util-linux) on this machine's `PATH`.
fn unshare() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join("unshare"))
        .find(|path| path.is_file())
}

/// Run `command` with `stdin`, and wait for it, at most [`TIMEOUT`]: its
/// stdout, stderr and exit code.
fn spawn(mut command: Command, stdin: &str) -> (String, String, Option<i32>) {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the native executable should start");
    let mut input = child.stdin.take().expect("a stdin pipe");
    let stdin = stdin.to_string();
    // A program that never reads its stdin may exit before it is written.
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(stdin.as_bytes());
    });
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = read(Box::new(child.stdout.take().expect("a stdout pipe")));
    let stderr = read(Box::new(child.stderr.take().expect("a stderr pipe")));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("should wait for the process") {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            panic!("the native executable ran for more than {TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let _ = writer.join();
    let stdout = stdout.join().expect("the stdout reader");
    let stderr = stderr.join().expect("the stderr reader");
    (stdout, stderr, status.code())
}

/// Every entry under `dir`, by path relative to the root, never following
/// a symlink.
fn read_tree(dir: &Path, prefix: &str, tree: &mut BTreeMap<String, Entry>) {
    let entries = std::fs::read_dir(dir).expect("should list a sandbox directory");
    for entry in entries {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let path = entry.path();
        let kind = std::fs::symlink_metadata(&path).expect("should stat the entry");
        if kind.file_type().is_symlink() {
            let target = std::fs::read_link(&path).expect("should read the symlink");
            tree.insert(name, Entry::Link(target.display().to_string()));
        } else if kind.is_dir() {
            tree.insert(name.clone(), Entry::Dir);
            read_tree(&path, &format!("{name}/"), tree);
        } else {
            let text = kind
                .is_file()
                .then(|| std::fs::read(&path).ok())
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok());
            tree.insert(name, text.map_or(Entry::Other, Entry::File));
        }
    }
}
