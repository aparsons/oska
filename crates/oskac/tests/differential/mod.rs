//! Seam 1 of the native backend (spec #125): the **differential
//! harness**.
//!
//! A [`Case`] is one program, a single file or a package, with its
//! arguments, stdin, environment and starting directory tree. [`run`]
//! runs it twice, in two identical temporary directories:
//!
//! - on the interpreter's real host, as `oskac run --interpret`;
//! - as a native executable written by `oskac build` outside the
//!   directory, so that it does not show up in the tree.
//!
//! A case with run flags ([`Case::with_run_flags`], such as a run limit,
//! which only `oskac run` takes) runs natively as `oskac run` with them
//! instead, and on the interpreter with them too.
//!
//! [`assert_same`] then requires byte-identical stdout and stderr (a
//! panic report included), the same exit code, and the same directory
//! tree afterwards. The program is named by the same relative path on both
//! sides, so a panic's location reads the same.
//!
//! It is compiled only where native executables run: aarch64 and x86_64
//! Linux and macOS, each running what it builds for its own
//! platform. Every target
//! with a backend must compile every case (spec #153): the host also
//! builds it with `--target` for each other one, which it cannot run, so
//! an aarch64 host still fails when x86_64 refuses a program, and a
//! refusal (`E0390`) fails on every host.
//!
//! To add a differential test, write a `#[test]` that builds a [`Case`]
//! and calls [`assert_same`], which also returns the outcome for any
//! assertion of what the program should print.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oska_syntax::Frames;

/// One program and everything it starts with.
pub struct Case {
    /// The name of the case, unique among the tests: the temporary
    /// directories are named after it.
    name: String,
    /// The program's files, by path relative to the working directory.
    files: Vec<(String, String)>,
    /// What the program is named by on the command line: a file, or a
    /// package directory.
    target: String,
    args: Vec<String>,
    stdin: String,
    env: Vec<(String, String)>,
    /// Run on each starting tree after its files are written: for what
    /// files alone cannot set up, such as symlinks and hard links.
    setup: Option<fn(&Path)>,
    /// Flags for `oskac run` on both backends, such as `--max-output=64`.
    run_flags: Vec<String>,
}

impl Case {
    /// A single-file program, `main.osk`.
    pub fn file(name: &str, src: &str) -> Case {
        Case {
            name: name.to_string(),
            files: vec![("main.osk".to_string(), src.to_string())],
            target: "main.osk".to_string(),
            args: Vec::new(),
            stdin: String::new(),
            env: Vec::new(),
            setup: None,
            run_flags: Vec::new(),
        }
    }

    /// A single-file program at `path`, relative to the working directory,
    /// which it is run by.
    pub fn file_at(name: &str, path: &str, src: &str) -> Case {
        Case {
            files: vec![(path.to_string(), src.to_string())],
            target: path.to_string(),
            ..Case::file(name, src)
        }
    }

    /// A package in the directory `app`, from its files relative to it,
    /// such as `oska.toml` and `src/main.osk`.
    pub fn package(name: &str, files: &[(&str, &str)]) -> Case {
        Case {
            name: name.to_string(),
            files: files
                .iter()
                .map(|(path, text)| (format!("app/{path}"), text.to_string()))
                .collect(),
            target: "app".to_string(),
            args: Vec::new(),
            stdin: String::new(),
            env: Vec::new(),
            setup: None,
            run_flags: Vec::new(),
        }
    }

    /// A package in the directory `dir`, from its files relative to the
    /// working directory (each under `dir/`).
    pub fn package_at(name: &str, dir: &str, files: Vec<(String, String)>) -> Case {
        Case {
            files,
            target: dir.to_string(),
            ..Case::file(name, "")
        }
    }

    /// What the program is named by on the command line.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The program's files and the starting tree's, by relative path.
    pub fn files(&self) -> &[(String, String)] {
        &self.files
    }

    /// A file in the starting directory tree, besides the program.
    pub fn with_file(mut self, path: &str, text: &str) -> Case {
        self.files.push((path.to_string(), text.to_string()));
        self
    }

    /// Run `setup` on each starting tree, after its files are written.
    pub fn with_setup(mut self, setup: fn(&Path)) -> Case {
        self.setup = Some(setup);
        self
    }

    pub fn with_args(mut self, args: &[&str]) -> Case {
        self.args = args.iter().map(|a| a.to_string()).collect();
        self
    }

    pub fn with_stdin(mut self, stdin: &str) -> Case {
        self.stdin = stdin.to_string();
        self
    }

    /// Run the program with `oskac run` and `flags` on both backends.
    pub fn with_run_flags(mut self, flags: &[&str]) -> Case {
        self.run_flags = flags.iter().map(|f| f.to_string()).collect();
        self
    }

    pub fn with_env(mut self, name: &str, value: &str) -> Case {
        self.env.push((name.to_string(), value.to_string()));
        self
    }
}

/// What one run of a program did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    /// `None` when a signal killed it.
    pub exit_code: Option<i32>,
    /// Every file under the working directory afterwards, by relative
    /// path, with its contents (`<not UTF-8: HEX>` for bytes that are not
    /// text); a directory has `None`.
    pub tree: BTreeMap<String, Option<String>>,
}

/// The target platform this host builds and runs native executables
/// for: its own.
pub fn host_target() -> oska_native::Target {
    oska_native::Target::host().expect("this host runs native executables")
}

/// `each` applied to every item, in order, with the items spread over
/// as many threads as the host has cores.
///
/// A test that goes through many cases one after another uses one core:
/// on the Intel Mac CI runner that made one test of `native_prelude` take
/// over a minute. Cases do not share state: each runs in directories
/// named after it, and [`Case`] names are unique. A case that fails
/// prints its own message; the other threads run the remaining cases, and
/// then the test fails.
pub fn in_parallel<T: Sync, R: Send>(items: &[T], each: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else { break };
                let result = each(item);
                results.lock().expect("no thread panics holding the lock")[index] = Some(result);
            });
        }
    });
    results
        .into_inner()
        .expect("no thread panics holding the lock")
        .into_iter()
        .map(|result| result.expect("every item was run"))
        .collect()
}

/// Run `case` on both backends and require the same outcome, and build
/// it for every other target ([`cross_build`]). Returns the outcome.
pub fn assert_same(case: &Case) -> Outcome {
    let root = case_root(case);
    let interpreted = interpret(case, &root);
    let native = native(case, &root);
    assert!(
        interpreted == native,
        "case `{}`: the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}",
        case.name
    );
    cross_build(case, &root);
    native
}

/// [`assert_same`] for runaway recursion (`P0021`), given `deepest`: the
/// same recursion, nesting exactly as many calls as the limit allows
/// (`oska_run::MAX_CALL_DEPTH`). Native code always reaches the limit,
/// and the interpreter reaches it when `deepest` runs on it: then the
/// two outcomes are the same, byte for byte. A debug build's interpreter
/// may run out of its own stack first, on some hosts and for some calls
/// (`docs/runtime.md`, `P0021`), which `deepest` panicking on it shows:
/// then its outcome is the same but for the count of frames left out,
/// which is at least one and at most native code's. Returns both
/// outcomes, the interpreter's first.
pub fn assert_same_at_depth_limit(case: &Case, deepest: &Case) -> (Outcome, Outcome) {
    let root = case_root(deepest);
    let deepest_native = native(deepest, &root);
    assert_eq!(
        deepest_native.exit_code,
        Some(0),
        "case `{}` nests calls past the limit: {deepest_native:#?}",
        deepest.name
    );
    let deepest_interpreted = interpret(deepest, &root);
    let (interpreted, native) = run(case);
    let differ = format!(
        "case `{}`: the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}",
        case.name
    );
    if deepest_interpreted.exit_code == Some(0) {
        assert!(interpreted == native, "{differ}");
        return (interpreted, native);
    }
    // The interpreter ran out of its own stack before the limit.
    assert!(
        deepest_interpreted.exit_code == Some(101)
            && deepest_interpreted.stderr.contains("error[P0021]"),
        "case `{}` fails on the interpreter: {deepest_interpreted:#?}",
        deepest.name
    );
    assert!(
        without_omitted_counts(&interpreted) == without_omitted_counts(&native),
        "{differ}"
    );
    let (fewer, all) = (
        omitted_counts(&interpreted.stderr),
        omitted_counts(&native.stderr),
    );
    assert!(
        fewer.len() == all.len() && fewer.iter().zip(&all).all(|(i, n)| 0 < *i && i <= n),
        "{differ}"
    );
    (interpreted, native)
}

/// The counts of frames left out in `stderr`: the human report's
/// (`    ... n more frames`) and the JSON's (`frames_omitted`).
fn omitted_counts(stderr: &str) -> Vec<usize> {
    stderr
        .lines()
        .filter_map(|line| {
            Frames::omitted_in(line).or_else(|| {
                let (_, tail) = line.split_once(FRAMES_OMITTED)?;
                let digits =
                    tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                tail[..digits].parse().ok()
            })
        })
        .collect()
}

/// The JSON's key for the count of frames left out, and its colon.
const FRAMES_OMITTED: &str = "\"frames_omitted\":";

/// `outcome` with each count of frames left out in its stderr
/// ([`omitted_counts`]) masked as `N`.
fn without_omitted_counts(outcome: &Outcome) -> Outcome {
    let mut masked = outcome.clone();
    masked.stderr = outcome
        .stderr
        .lines()
        .map(|line| {
            if Frames::omitted_in(line).is_some() {
                return "    ... N more frames\n".to_string();
            }
            match line.split_once(FRAMES_OMITTED) {
                Some((head, tail)) => {
                    let rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
                    format!("{head}{FRAMES_OMITTED}N{rest}\n")
                }
                None => format!("{line}\n"),
            }
        })
        .collect();
    masked
}

/// Build `case` in `root` with `--target` for every target with a
/// backend but the host's, and fail on a refusal: what a host that cannot
/// run another target's code can check of it.
fn cross_build(case: &Case, root: &Path) {
    for target in oska_native::Target::ALL {
        if target != host_target() {
            let cross = root.join(target.name());
            let dir = fresh_tree(&cross.join("tree"), case);
            if let Err(report) = try_build(&cross, &dir, case, Some(target)) {
                panic!(
                    "case `{}`: oskac build --target {} failed\n{report}",
                    case.name,
                    target.name()
                );
            }
        }
    }
}

/// Run `case` on the interpreter and natively, and build it for every
/// other target ([`cross_build`]): the two outcomes.
pub fn run(case: &Case) -> (Outcome, Outcome) {
    let root = case_root(case);
    let interpreted = interpret(case, &root);
    let native = native(case, &root);
    cross_build(case, &root);
    (interpreted, native)
}

/// Run `case` on the interpreter, in `root/interpreted`.
fn interpret(case: &Case, root: &Path) -> Outcome {
    let interpreted_dir = fresh_tree(&root.join("interpreted"), case);
    let mut interpreter = oskac(&interpreted_dir);
    interpreter
        .args(["run", "--interpret"])
        .args(&case.run_flags)
        .arg(&case.target)
        .arg("--")
        .args(&case.args);
    outcome(interpreter, case, &interpreted_dir)
}

/// Run `case` natively only, and build it for every other target
/// ([`cross_build`]): for what the interpreter cannot show, such as its
/// exact call-depth limit, which a debug build of it may not reach.
pub fn run_native(case: &Case) -> Outcome {
    let root = case_root(case);
    cross_build(case, &root);
    native(case, &root)
}

/// The directory `case` runs in, emptied.
fn case_root(case: &Case) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("differential")
        .join(&case.name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// Runs `exe` in `dir` and checks it never holds more than `mib` MiB
/// ([`within_memory`]).
pub fn run_within_memory(exe: &Path, dir: &Path, mib: u64) -> std::process::Output {
    within_memory(exe, &[], dir, mib)
}

/// Runs `program` with `args` in `dir` and checks it never holds more
/// than `mib` MiB. Linux caps the address space with `ulimit -v`, so a
/// run that needs more fails; macOS cannot (`ulimit -v` fails there), so
/// on macOS the peak resident memory `time -l` reports is checked after
/// the run instead, and its report follows the program's on stderr.
pub fn within_memory(program: &Path, args: &[&str], dir: &Path, mib: u64) -> std::process::Output {
    let script = if cfg!(target_os = "macos") {
        "/usr/bin/time -l \"$0\" \"$@\"".to_string()
    } else {
        format!("ulimit -v {} && exec \"$0\" \"$@\"", mib * 1024)
    };
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("sh should run the program");
    if cfg!(target_os = "macos") {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let peak: Option<u64> = stderr
            .lines()
            .find(|line| line.contains("maximum resident set size"))
            .and_then(|line| line.split_whitespace().next()?.parse().ok());
        match peak {
            Some(peak) => assert!(peak < mib << 20, "peak resident memory {peak} bytes"),
            None => assert!(
                !output.status.success(),
                "no peak memory in `time -l`'s report: {stderr}"
            ),
        }
    }
    output
}

/// Build `case` natively, without running it, and for every other
/// target ([`cross_build`]): the executable, and the directory to run it
/// in, holding `case`'s files.
pub fn build_native(case: &Case) -> (PathBuf, PathBuf) {
    let root = case_root(case);
    cross_build(case, &root);
    let native_dir = fresh_tree(&root.join("native"), case);
    (build(case, &root, &native_dir), native_dir)
}

/// Build `case` and run the executable in `root/native`.
fn native(case: &Case, root: &Path) -> Outcome {
    let native_dir = fresh_tree(&root.join("native"), case);
    if !case.run_flags.is_empty() {
        let mut run = oskac(&native_dir);
        run.arg("run")
            .args(&case.run_flags)
            .args(["--no-cache", &case.target, "--"])
            .args(&case.args);
        return outcome(run, case, &native_dir);
    }
    let exe = build(case, root, &native_dir);
    let mut program = Command::new(&exe);
    program.current_dir(&native_dir).args(&case.args);
    outcome(program, case, &native_dir)
}

/// Build `case`, from `native_dir`, to `root/bin/program`.
fn build(case: &Case, root: &Path, native_dir: &Path) -> PathBuf {
    try_build(root, native_dir, case, None)
        .unwrap_or_else(|report| panic!("case `{}`: oskac build failed\n{report}", case.name))
}

/// [`build`], or what `oskac build` printed when it refused `case`: for
/// `target`, or by default for the host's ([`host_target`]).
fn try_build(
    root: &Path,
    native_dir: &Path,
    case: &Case,
    target: Option<oska_native::Target>,
) -> Result<PathBuf, String> {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("should create the bin directory");
    let target = target.unwrap_or_else(host_target);
    let exe = bin.join(format!("program{}", target.suffix()));
    let mut build = oskac(native_dir);
    build.arg("build").arg(&case.target).arg("-o").arg(&exe);
    build.args(["--target", target.name()]);
    let build = build.output().expect("oskac build should run");
    if !build.status.success() {
        return Err(format!(
            "{}{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    Ok(exe)
}

fn oskac(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command.current_dir(dir);
    command
}

/// A new directory at `dir` holding `case`'s files.
fn fresh_tree(dir: &Path, case: &Case) -> PathBuf {
    std::fs::create_dir_all(dir).expect("should create the case directory");
    for (path, text) in &case.files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    if let Some(setup) = case.setup {
        setup(dir);
    }
    dir.to_path_buf()
}

/// Run `command` with `case`'s stdin and only its environment, and
/// observe it.
fn outcome(mut command: Command, case: &Case, dir: &Path) -> Outcome {
    command
        .env_clear()
        .envs(case.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the program should start");
    // Stdin is written on its own thread while stdout and stderr are read:
    // a program that echoes its input fills its stdout pipe (64 KiB on a
    // 4 KiB-page kernel) and waits, and writing all of stdin first would
    // then wait on it forever. A program that never reads its stdin may
    // close it first, so the write's error is ignored.
    let writer = child.stdin.take().map(|mut stdin| {
        let bytes = case.stdin.clone().into_bytes();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let output = child.wait_with_output().expect("the program should finish");
    if let Some(writer) = writer {
        writer.join().expect("the stdin writer should not panic");
    }
    let mut tree = BTreeMap::new();
    read_tree(dir, "", &mut tree);
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        tree,
    }
}

/// A file's contents as the tree records them: its text, or, for bytes
/// that are not UTF-8, every byte in hexadecimal, so that two files that
/// differ in any byte never look the same.
fn contents(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| {
        let hex: String = e.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        format!("<not UTF-8: {hex}>")
    })
}

fn read_tree(dir: &Path, prefix: &str, tree: &mut BTreeMap<String, Option<String>>) {
    let entries = std::fs::read_dir(dir).expect("should list the directory");
    for entry in entries {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let path = entry.path();
        let kind = std::fs::symlink_metadata(&path).expect("should stat the entry");
        // A symlink is recorded by its target, never followed; anything
        // that is neither a file nor a directory (a FIFO) by its name.
        if kind.file_type().is_symlink() {
            let target = std::fs::read_link(&path).expect("should read the symlink");
            tree.insert(format!("{name}@"), Some(target.display().to_string()));
        } else if !kind.is_dir() && !kind.is_file() {
            tree.insert(format!("{name}|"), None);
        } else if path.is_dir() {
            tree.insert(format!("{name}/"), None);
            read_tree(&path, &format!("{name}/"), tree);
        } else {
            let bytes = std::fs::read(&path).expect("should read the file");
            tree.insert(name, Some(contents(bytes)));
        }
    }
}
