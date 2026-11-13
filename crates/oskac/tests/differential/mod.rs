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
//! [`assert_same`] then requires byte-identical stdout and stderr (a
//! panic report included), the same exit code, and the same directory
//! tree afterwards. The program is named by the same relative path on both
//! sides, so a panic's location reads the same.
//!
//! It is compiled only where native executables run: aarch64 and x86_64
//! Linux, each building for its own platform. On x86_64, whose backend
//! is new (spec #153), a program the backend refuses (`E0390`) is
//! reported on stderr and compared on the interpreter alone
//! ([`REPORT_REFUSALS`]), until x86_64 Linux is native by default;
//! [`assert_compiled_same`] instead fails on a refusal on every host, and
//! builds the program for every other target too, as [`build_compiled`]
//! and [`run_compiled_native`] do for a case run natively only.
//!
//! To add a differential test, write a `#[test]` that builds a [`Case`]
//! and calls [`assert_same`], which also returns the outcome for any
//! assertion of what the program should print.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    /// path, with its contents; a directory has `None`.
    pub tree: BTreeMap<String, Option<String>>,
}

/// The target platform this host builds and runs native executables
/// for: its own.
pub fn host_target() -> oska_native::Target {
    if cfg!(target_arch = "x86_64") {
        oska_native::Target::LinuxX86_64
    } else {
        oska_native::Target::LinuxAarch64
    }
}

/// Whether a program `oskac build` refuses with `E0390` is reported
/// rather than failed: on x86_64 Linux, until it is native by default
/// (spec #153's default-switch ticket).
pub const REPORT_REFUSALS: bool = cfg!(target_arch = "x86_64");

/// Whether `oskac build`'s refusal of the program `name`, which printed
/// `report`, is one to report rather than fail ([`REPORT_REFUSALS`]):
/// after saying so on stderr.
pub fn refusal_reported(name: &str, report: &str) -> bool {
    if !REPORT_REFUSALS || !report.contains("E0390") {
        return false;
    }
    eprintln!(
        "case `{name}`: the x86_64 backend cannot compile it yet, so only the \
         interpreter ran it:\n{report}"
    );
    true
}

/// Run `case` on both backends and require the same outcome. Returns it:
/// the interpreter's alone when the backend's refusal is reported
/// ([`refusal_reported`]).
pub fn assert_same(case: &Case) -> Outcome {
    let root = case_root(case);
    let interpreted = interpret(case, &root);
    let native = match try_native(case, &root) {
        Ok(native) => native,
        Err(report) if refusal_reported(&case.name, &report) => return interpreted,
        Err(report) => panic!("case `{}`: oskac build failed\n{report}", case.name),
    };
    assert!(
        interpreted == native,
        "case `{}`: the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}",
        case.name
    );
    native
}

/// [`assert_same`] for a program every target must compile: `oskac
/// build` refusing it (`E0390`) fails on every host, never falling back
/// to the interpreter ([`REPORT_REFUSALS`]). The host runs its own
/// target's executable, and builds it for every other target with a
/// backend as well, so an aarch64 host, which cannot run x86_64 code,
/// still fails when x86_64 refuses the program.
pub fn assert_compiled_same(case: &Case) -> Outcome {
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

/// Run `case` on the interpreter and natively: the two outcomes.
pub fn run(case: &Case) -> (Outcome, Outcome) {
    let root = case_root(case);
    let interpreted = interpret(case, &root);
    (interpreted, native(case, &root))
}

/// Run `case` on the interpreter, in `root/interpreted`.
fn interpret(case: &Case, root: &Path) -> Outcome {
    let interpreted_dir = fresh_tree(&root.join("interpreted"), case);
    let mut interpreter = oskac(&interpreted_dir);
    interpreter
        .args(["run", "--interpret"])
        .arg(&case.target)
        .arg("--")
        .args(&case.args);
    outcome(interpreter, case, &interpreted_dir)
}

/// Run `case` natively only: for what the interpreter cannot show, such
/// as its exact call-depth limit, which a debug build of it may not
/// reach.
pub fn run_native(case: &Case) -> Outcome {
    native(case, &case_root(case))
}

/// The directory `case` runs in, emptied.
fn case_root(case: &Case) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("differential")
        .join(&case.name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// Build `case` natively, without running it: the executable, and the
/// directory to run it in, holding `case`'s files.
pub fn build_native(case: &Case) -> (PathBuf, PathBuf) {
    let root = case_root(case);
    let native_dir = fresh_tree(&root.join("native"), case);
    (build(case, &root, &native_dir), native_dir)
}

/// [`build_native`] for a program every target must compile, as
/// [`assert_compiled_same`] requires: it is built for every other target
/// with a backend as well, and a refusal fails on every host.
pub fn build_compiled(case: &Case) -> (PathBuf, PathBuf) {
    let root = case_root(case);
    cross_build(case, &root);
    let native_dir = fresh_tree(&root.join("native"), case);
    (build(case, &root, &native_dir), native_dir)
}

/// [`run_native`] for a program every target must compile, as
/// [`build_compiled`] builds it.
pub fn run_compiled_native(case: &Case) -> Outcome {
    let root = case_root(case);
    cross_build(case, &root);
    native(case, &root)
}

/// Build `case` and run the executable in `root/native`.
fn native(case: &Case, root: &Path) -> Outcome {
    try_native(case, root)
        .unwrap_or_else(|report| panic!("case `{}`: oskac build failed\n{report}", case.name))
}

/// [`native`], or what `oskac build` printed when it refused `case`.
fn try_native(case: &Case, root: &Path) -> Result<Outcome, String> {
    let native_dir = fresh_tree(&root.join("native"), case);
    let exe = try_build(root, &native_dir, case, None)?;
    let mut program = Command::new(&exe);
    program.current_dir(&native_dir).args(&case.args);
    Ok(outcome(program, case, &native_dir))
}

/// Build `case`, from `native_dir`, to `root/bin/program`.
fn build(case: &Case, root: &Path, native_dir: &Path) -> PathBuf {
    try_build(root, native_dir, case, None)
        .unwrap_or_else(|report| panic!("case `{}`: oskac build failed\n{report}", case.name))
}

/// [`build`], or what `oskac build` printed when it refused `case`: for
/// `target`, or by default for the host's.
fn try_build(
    root: &Path,
    native_dir: &Path,
    case: &Case,
    target: Option<oska_native::Target>,
) -> Result<PathBuf, String> {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("should create the bin directory");
    let exe = bin.join("program");
    let mut build = oskac(native_dir);
    build.arg("build").arg(&case.target).arg("-o").arg(&exe);
    if let Some(target) = target {
        build.args(["--target", target.name()]);
    }
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
            tree.insert(name, Some(String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}
