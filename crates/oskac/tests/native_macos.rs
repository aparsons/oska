//! The first macOS executables (spec #154, ticket #169): small programs
//! built with `oskac build --target macos-aarch64`, on every host.
//!
//! On an Apple silicon Mac (CI's `macos-latest`) each executable must
//! pass macOS's own signature verification (`codesign --verify
//! --strict`), and running it must print, write and exit as `oskac run
//! --interpret` does. Elsewhere only the builds are checked: no other
//! host can run them, and `oska-native`'s format tests read their bytes.
//!
//! The programs stay inside what this ticket brings up: integers,
//! calls, exit codes and standard output, through `_start`'s arguments,
//! `std.rt`'s start-up and libSystem's calls, and one failing call, whose
//! `errno` becomes the `IoError` the program reports. The rest of the
//! runtime is tickets #170 to #172.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A program, its name, and the arguments it runs with.
struct Program {
    name: &'static str,
    src: &'static str,
    args: &'static [&'static str],
}

const PROGRAMS: &[Program] = &[
    Program {
        name: "hello",
        src: "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"Hello from Oska!\")\n}\n",
        args: &[],
    },
    Program {
        name: "integers",
        src: r#"fn fib(n: Int) -> Int {
    if n < 2 {
        return n
    }
    fib(n - 1) + fib(n - 2)
}

fn gcd(a: Int, b: Int) -> Int {
    if b == 0 {
        return a
    }
    gcd(b, a % b)
}

fn main(io: Stdout) -> Result<(), IoError> {
    var sum = 0
    var i = 1
    while i <= 100 {
        sum = sum + i * i
        i = i + 1
    }
    io.println("sum {sum}")?
    let f = fib(20)
    io.println("fib {f}")?
    let g = gcd(1071, 462)
    io.println("gcd {g}")?
    let quotient = -17 / 5
    let remainder = -17 % 5
    io.println("division {quotient} {remainder}")?
    let large = 9223372036854775807 - 1
    io.println("large {large}")?
    Ok(())
}
"#,
        args: &[],
    },
    Program {
        name: "arguments",
        src: r#"fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    var i = 1
    var more = true
    while more {
        match args.get(i) {
            Some(arg) => {
                io.println("[{i}] {arg}")?
                i = i + 1
            }
            None => {
                more = false
            }
        }
    }
    let count = i - 1
    io.println("{count} arguments")
}
"#,
        args: &["one", "two words", "three"],
    },
    // `main` returns the `Err` of an `openat` that failed with `ENOENT`:
    // its message on stderr, and exit code 1.
    Program {
        name: "exit_code",
        src: r#"fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    io.println("before")?
    let text = fs.read_only().restrict_to(".").read_to_string(Path.new("missing.txt"))?
    io.println(text)
}
"#,
        args: &[],
    },
];

/// A new, empty directory for `name` in the test `test`: each test has
/// its own, since tests run in parallel.
fn workspace(test: &str, name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("native_macos")
        .join(test)
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    dir
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// What a run printed, wrote and exited with.
fn outcome(output: &Output) -> (String, String, Option<i32>) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// Build `program` for `macos-aarch64` in a fresh directory: the
/// directory, under the test `test`'s, and the executable's path.
fn build(test: &str, program: &Program) -> (PathBuf, PathBuf) {
    let dir = workspace(test, program.name);
    let source = format!("{}.osk", program.name);
    std::fs::write(dir.join(&source), program.src).expect("should write the program");
    let built = oskac(
        &dir,
        &[
            "build",
            &source,
            "--target",
            "macos-aarch64",
            "-o",
            "program",
        ],
    );
    assert!(
        built.status.success(),
        "`{}` does not build for macos-aarch64: {built:?}",
        program.name
    );
    let exe = dir.join("program");
    let bytes = std::fs::read(&exe).expect("the executable");
    // MH_MAGIC_64, CPU_TYPE_ARM64.
    assert_eq!(&bytes[..8], [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01]);
    (dir, exe)
}

#[test]
fn small_programs_build_for_macos_aarch64_on_every_host() {
    for program in PROGRAMS {
        build("build", program);
    }
}

/// On an Apple silicon Mac: macOS accepts each signature, and each
/// program does what the interpreter does.
#[test]
fn small_programs_run_on_apple_silicon_as_the_interpreter_runs_them() {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        eprintln!("skipped: only an Apple silicon Mac runs macos-aarch64 executables");
        return;
    }
    for program in PROGRAMS {
        let (dir, exe) = build("run", program);
        let verified = Command::new("codesign")
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(&exe)
            .output()
            .expect("codesign should run");
        assert!(
            verified.status.success(),
            "macOS refuses `{}`'s signature: {}",
            program.name,
            String::from_utf8_lossy(&verified.stderr)
        );
        let native = Command::new(&exe)
            .args(program.args)
            .current_dir(&dir)
            .output()
            .expect("the executable should start");
        let source = format!("{}.osk", program.name);
        let mut args = vec!["run", "--interpret", source.as_str(), "--"];
        args.extend_from_slice(program.args);
        let interpreted = oskac(&dir, &args);
        assert_eq!(
            outcome(&native),
            outcome(&interpreted),
            "`{}`: native (left) and interpreted (right) differ",
            program.name
        );
    }
}
