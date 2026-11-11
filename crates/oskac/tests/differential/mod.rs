//! Seam 1 of the native backend (spec #125): the **differential
//! harness**.
//!
//! A [`Case`] is one program, a single file or a package, with its
//! arguments, stdin, environment and starting directory tree. [`run`]
//! runs it twice, in two identical temporary directories:
//!
//! - on the interpreter's real host, as `oskac run` (today's default;
//!   once native is the default, `oskac run --interpret`);
//! - as a native executable written by `oskac build` outside the
//!   directory, so that it does not show up in the tree.
//!
//! [`assert_same`] then requires byte-identical stdout and stderr (a
//! panic report included), the same exit code, and the same directory
//! tree afterwards. The program is named by the same relative path on both
//! sides, so a panic's location reads the same.
//!
//! It is compiled only where native executables run: aarch64 Linux.
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

/// Run `case` on both backends and require the same outcome. Returns it.
pub fn assert_same(case: &Case) -> Outcome {
    let (interpreted, native) = run(case);
    assert!(
        interpreted == native,
        "case `{}`: the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}",
        case.name
    );
    native
}

/// Run `case` on the interpreter and natively: the two outcomes.
pub fn run(case: &Case) -> (Outcome, Outcome) {
    let root = case_root(case);
    let interpreted_dir = fresh_tree(&root.join("interpreted"), case);
    let mut interpreter = oskac(&interpreted_dir);
    interpreter
        .arg("run")
        .arg(&case.target)
        .arg("--")
        .args(&case.args);
    let interpreted = outcome(interpreter, case, &interpreted_dir);
    (interpreted, native(case, &root))
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

/// Build `case` and run the executable in `root/native`.
fn native(case: &Case, root: &Path) -> Outcome {
    let native_dir = fresh_tree(&root.join("native"), case);
    let exe = build(case, root, &native_dir);
    let mut program = Command::new(&exe);
    program.current_dir(&native_dir).args(&case.args);
    outcome(program, case, &native_dir)
}

/// Build `case`, from `native_dir`, to `root/bin/program`.
fn build(case: &Case, root: &Path, native_dir: &Path) -> PathBuf {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("should create the bin directory");
    let exe = bin.join("program");
    let build = oskac(native_dir)
        .arg("build")
        .arg(&case.target)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("oskac build should run");
    assert!(
        build.status.success(),
        "case `{}`: oskac build failed\n{}{}",
        case.name,
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    exe
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
    if let Some(mut stdin) = child.stdin.take() {
        // A program that never reads its stdin may close it first.
        let _ = stdin.write_all(case.stdin.as_bytes());
    }
    let output = child.wait_with_output().expect("the program should finish");
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
