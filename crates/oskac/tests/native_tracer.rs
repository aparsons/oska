//! The native tracer (ticket #128): the first programs `oskac build`
//! compiles, run on both backends through the differential harness
//! (seam 1). Native executables run only on aarch64 and x86_64 Linux, so
//! these tests exist only there; `build_golden.rs` covers `oskac build`
//! everywhere.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

#[test]
fn hello_world_prints_the_same_on_both_backends() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/hello.osk"
    ))
    .expect("the hello example should exist");
    let outcome = assert_same(&Case::file("hello", &src));
    assert_eq!(outcome.stdout, "Hello from Oska!\n");
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn a_main_without_a_result_exits_0_after_printing_to_both_streams() {
    let src = "fn main(out: Stdout, err: Stderr) {\n    \
               let _ = out.println(\"to stdout\")\n    \
               let _ = err.println(\"to stderr, ünïcode and all\")\n    \
               let _ = out.println(\"\")\n}\n";
    let outcome = assert_same(&Case::file("both_streams", src));
    assert_eq!(outcome.stdout, "to stdout\n\n");
    assert_eq!(outcome.stderr, "to stderr, ünïcode and all\n");
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn an_err_from_main_prints_its_display_text_to_stderr_and_exits_1() {
    let src = "enum AppError {\n    Broken,\n    Lost,\n}\n\n\
               impl Display for AppError {\n    \
               fn to_string(self) -> String {\n        \"it broke\"\n    }\n}\n\n\
               fn main(io: Stdout) -> Result<(), AppError> {\n    \
               let _ = io.println(\"before\")\n    \
               Err(AppError.Lost)\n}\n";
    let outcome = assert_same(&Case::file("err_from_main", src));
    assert_eq!(outcome.stdout, "before\n");
    assert_eq!(outcome.stderr, "it broke\n");
    assert_eq!(outcome.exit_code, Some(1));
}

#[test]
fn a_long_line_is_written_in_full() {
    let line = "0123456789abcdef".repeat(8192);
    let src =
        format!("fn main(io: Stdout) -> Result<(), IoError> {{\n    io.println(\"{line}\")\n}}\n");
    let outcome = assert_same(&Case::file("long_line", &src));
    assert_eq!(outcome.stdout, format!("{line}\n"));
}

#[test]
fn a_package_builds_and_runs_the_same() {
    let outcome = assert_same(&Case::package(
        "package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/main.osk",
                "use pkg.greet\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    \
                 greet.hello(io)\n}\n",
            ),
            (
                "src/greet.osk",
                "pub fn hello(io: Stdout) -> Result<(), IoError> {\n    \
                 io.println(\"hello from a module\")\n}\n",
            ),
        ],
    ));
    assert_eq!(outcome.stdout, "hello from a module\n");
}

/// A little-endian field of `bytes`.
fn field(bytes: &[u8], at: usize, size: usize) -> u64 {
    bytes[at..at + size]
        .iter()
        .rev()
        .fold(0, |acc, &b| acc << 8 | u64::from(b))
}

#[test]
fn the_kernel_maps_each_segment_with_only_its_own_permissions() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // A program that waits on stdin, so its mappings can be read while it
    // runs.
    let src = "fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {\n    \
               let _ = input.read_line()?\n    io.println(\"done\")\n}\n";
    let (exe, dir) = differential::build_native(&Case::file("maps", src));
    let elf = std::fs::read(&exe).expect("the executable");
    // (file offset, file size, permissions) per PT_LOAD.
    let phoff = field(&elf, 32, 8) as usize;
    let segments: Vec<(u64, u64, &str)> = (0..field(&elf, 56, 2) as usize)
        .map(|i| phoff + 56 * i)
        .filter(|&h| field(&elf, h, 4) == 1)
        .map(|h| {
            let flags = match field(&elf, h + 4, 4) {
                4 => "r--",
                5 => "r-x",
                6 => "rw-",
                other => panic!("unexpected segment flags {other}"),
            };
            (field(&elf, h + 8, 8), field(&elf, h + 32, 8), flags)
        })
        .collect();
    let mut child = Command::new(&exe)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
    // Wait until the kernel has loaded it (its maps name the executable).
    let path = std::fs::canonicalize(&exe).expect("a canonical path");
    let path = path.to_str().expect("a UTF-8 path");
    let maps_file = format!("/proc/{}/maps", child.id());
    let mut maps = String::new();
    for _ in 0..200 {
        maps = std::fs::read_to_string(&maps_file).unwrap_or_default();
        if maps.contains(path) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut seen = 0;
    for line in maps.lines().filter(|l| l.ends_with(path)) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (range, perms, offset) = (parts[0], &parts[1][..3], parts[2]);
        let (start, end) = range.split_once('-').expect("a range");
        let length = u64::from_str_radix(end, 16).expect("hex")
            - u64::from_str_radix(start, 16).expect("hex");
        let offset = u64::from_str_radix(offset, 16).expect("hex");
        // The file bytes this mapping shows lie within one segment (up to
        // the page that ends it: the target's largest, 4 KiB on x86_64 and
        // 64 KiB on aarch64), and carry that segment's permissions.
        let page: u64 = if cfg!(target_arch = "x86_64") {
            0x1000
        } else {
            0x1_0000
        };
        let owner = segments.iter().find(|(at, size, _)| {
            *at <= offset && offset + length <= (at + size).next_multiple_of(page)
        });
        match owner {
            Some((_, _, flags)) => assert_eq!(perms, *flags, "mapping `{line}`"),
            None => panic!("mapping `{line}` spans segments {segments:x?}"),
        }
        seen += 1;
    }
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"go\n")
        .expect("should write");
    let output = child.wait_with_output().expect("the program should end");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "done\n");
    assert!(seen >= 2, "the executable's mappings:\n{maps}");
}
