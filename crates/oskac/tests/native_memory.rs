//! Native memory and panics (ticket #160), on whichever of aarch64 and
//! x86_64 Linux the tests run: where `main`'s stack is and what guards
//! it, the allocator's size classes and large objects, reference counting
//! and copy-on-write, heap text, and panics with the interpreter's codes
//! and locations.
//!
//! The differential cases here use [`compiled_same`], which fails when
//! `oskac build` refuses the program on any host: memory and panics are
//! what every other x86_64 ticket stands on, so x86_64 may not fall back
//! to the interpreter for them, as the rest of the suite still may
//! (`differential::REPORT_REFUSALS`). Call depth at the `P0021` boundary
//! is `native_ints.rs`'s and `native_gaps.rs`'s, and deep drops in bounded
//! stack `native_data.rs`'s; those build natively on every host already.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use differential::Case;

/// `read`'s system-call number on the host, which a process waiting on
/// its stdin is blocked in.
const READ: u64 = if cfg!(target_arch = "x86_64") { 0 } else { 63 };

/// One line of `/proc/<pid>/maps`.
#[derive(Debug)]
struct Mapping {
    start: u64,
    end: u64,
    perms: String,
    /// The file or pseudo-file it shows, such as `[stack]`; empty for
    /// anonymous memory.
    name: String,
}

fn mappings(maps: &str) -> Vec<Mapping> {
    maps.lines()
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let (start, end) = parts[0].split_once('-').expect("a range");
            Mapping {
                start: u64::from_str_radix(start, 16).expect("hex"),
                end: u64::from_str_radix(end, 16).expect("hex"),
                perms: parts[1].to_string(),
                name: parts.get(5).copied().unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// The stack pointer of the process `pid` once it is blocked reading its
/// stdin, from `/proc/<pid>/syscall` (its last two fields are the stack
/// pointer and the program counter).
fn stack_pointer_in_read(pid: u32) -> u64 {
    let file = format!("/proc/{pid}/syscall");
    for _ in 0..500 {
        let line = std::fs::read_to_string(&file).unwrap_or_default();
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() > 2 && fields[0].parse() == Ok(READ) {
            let sp = fields[fields.len() - 2].trim_start_matches("0x");
            return u64::from_str_radix(sp, 16).expect("a hex stack pointer");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the program never blocked reading stdin");
}

#[test]
fn main_runs_on_a_stack_of_its_own_above_a_guard_region_and_nothing_is_writable_and_executable() {
    // `main` waits on stdin, so where its stack is can be read while it
    // runs.
    let src = "fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {\n    \
               let _ = input.read_line()?\n    io.println(\"done\")\n}\n";
    let (exe, dir) = differential::build_native(&Case::file("guarded_stack", src));
    let mut child = Command::new(&exe)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
    let sp = stack_pointer_in_read(child.id());
    let maps = std::fs::read_to_string(format!("/proc/{}/maps", child.id())).expect("its maps");
    let all = mappings(&maps);

    let stack = all
        .iter()
        .find(|m| m.start <= sp && sp < m.end)
        .unwrap_or_else(|| panic!("no mapping holds the stack pointer {sp:#x}:\n{maps}"));
    assert_eq!(stack.perms, "rw-p", "the stack: {stack:x?}\n{maps}");
    assert_eq!(
        stack.name, "",
        "main runs on the stack the runtime mapped, not the kernel's: {stack:x?}"
    );
    // Room for the 50,000 calls the language allows, each at least a
    // return address and a saved frame pointer.
    let calls = oska_native::MAX_CALL_DEPTH as u64;
    assert!(
        stack.end - stack.start >= calls * 16,
        "a stack too small for {calls} calls: {stack:x?}"
    );

    let guard = all
        .iter()
        .find(|m| m.end == stack.start)
        .unwrap_or_else(|| panic!("nothing is mapped right below the stack:\n{maps}"));
    assert_eq!(
        guard.perms, "---p",
        "below the stack, memory no access may touch: {guard:x?}\n{maps}"
    );
    // At least a whole page of either target (64 KiB on aarch64).
    assert!(
        guard.end - guard.start >= 0x1_0000,
        "a guard region of under 64 KiB: {guard:x?}"
    );

    for mapping in &all {
        let perms = mapping.perms.as_bytes();
        assert!(
            !(perms[1] == b'w' && perms[2] == b'x'),
            "writable and executable: {mapping:x?}\n{maps}"
        );
        if mapping.name == "[stack]" {
            assert_eq!(
                perms[2], b'-',
                "the kernel's stack is executable: {mapping:x?}"
            );
        }
    }

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"go\n")
        .expect("should write");
    let output = child.wait_with_output().expect("the program should end");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "done\n");
    assert_eq!(output.status.code(), Some(0));
}

/// Run `case` on both backends and require the same outcome: unlike
/// `differential::assert_same`, a program `oskac build` refuses fails
/// the test on every host. Returns the outcome.
fn compiled_same(case: &Case) -> differential::Outcome {
    let (interpreted, native) = differential::run(case);
    assert!(
        interpreted == native,
        "the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}"
    );
    native
}

#[test]
fn texts_on_each_side_of_every_size_class_and_large_ones_are_allocated_freed_and_reused() {
    // A text's block is a header word, its count and length, then its
    // bytes: lengths `block - 24` and `block - 23` fill a class's block
    // and spill into the next. Past 64 KiB an object is a mapping of its
    // own, and one freed is kept for the next.
    let src = r#"fn fill(n: Int, c: String) -> List<String> {
    var out: List<String> = []
    var i = 0
    while i < 8 {
        out.push(c.repeat(n))
        i = i + 1
    }
    out
}

fn check(io: Stdout, n: Int) {
    let first = fill(n, "x")
    let one = first.at(0)
    let grown = "{one}!"
    let second = fill(n, "y")
    let same = if first.at(7) == "x".repeat(n) && second.at(3) == "y".repeat(n) {
        "same"
    } else {
        "changed"
    }
    let length = grown.len()
    let _ = io.println("{n} {length} {same}")
}

fn main(io: Stdout) {
    var block = 32
    while block <= 65536 {
        check(io, block - 24)
        check(io, block - 23)
        block = block * 2
    }
    check(io, 100000)
    check(io, 300000)
    check(io, 100000)
    check(io, 1)
}
"#;
    let outcome = compiled_same(&Case::file("size_classes", src));
    let mut expected = String::new();
    let mut block = 32;
    while block <= 65536 {
        for n in [block - 24, block - 23] {
            expected += &format!("{n} {} same\n", n + 1);
        }
        block *= 2;
    }
    for n in [100000, 300000, 100000, 1] {
        expected += &format!("{n} {} same\n", n + 1);
    }
    assert_eq!(outcome.stdout, expected, "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn copies_share_their_objects_until_one_changes_and_text_grows_on_the_heap() {
    // Copies of a value share its objects, counted; a change to one copy
    // copies what it changes and never shows in the others (ADR 0001).
    // Text built in a loop and by doubling grows through every size
    // class into large objects.
    let src = r#"struct Bag { items: List<String>, name: String }

fn grow(b: Bag, item: String) -> Bag {
    var c = b
    c.items.push(item)
    c.name = "{c.name}+"
    c
}

fn main(io: Stdout) {
    let a = Bag(items: ["one", "two"], name: "a")
    let b = grow(a, "three")
    let c = grow(b, "four")
    var lists = [a.items, b.items]
    var more = lists
    more.push(c.items)
    var inner = more.at(0)
    inner.push("changed")
    let sizes = [a.items.len(), b.items.len(), c.items.len(), lists.len(), more.len(), inner.len()]
    var counts = ""
    for size in sizes {
        counts = "{counts} {size}"
    }
    let _ = io.println("{a.name} {b.name} {c.name}{counts}")

    let shared = "s".repeat(1000)
    var many: List<String> = []
    var i = 0
    while i < 2000 {
        many.push(shared)
        i = i + 1
    }
    var copy = many
    copy.push("last")
    many = []
    let copied = copy.len()
    let emptied = many.len()
    let first = copy.at(0).len()
    let _ = io.println("{copied} {emptied} {first}")

    var built = ""
    i = 0
    while i < 3000 {
        built = "{built}{i},"
        i = i + 1
    }
    var doubled = "ab"
    i = 0
    while i < 18 {
        doubled = "{doubled}{doubled}"
        i = i + 1
    }
    let length = built.len()
    let tail = built.slice(length - 10, length)
    let twice = doubled.len()
    let _ = io.println("{length} {tail} {twice}")
}
"#;
    let outcome = compiled_same(&Case::file("shared_and_grown", src));
    assert_eq!(
        outcome.stdout, "a a+ a++ 2 3 4 2 3 3\n2001 0 1000\n13890 2998,2999, 524288\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn every_panic_a_program_can_reach_reports_the_interpreters_code_and_location() {
    // Each program prints a line, which must reach stdout before the
    // panic's report reaches stderr, then panics.
    let cases: [(&str, &str, &str); 11] = [
        ("P0001", "", "    let x: U8 = 255\n    let y = x + 1"),
        (
            "P0002",
            "",
            "    let xs: List<Int> = [1, 2, 3]\n    let v = xs.at(3)",
        ),
        ("P0003", "", "    todo()"),
        ("P0004", "", "    let zero = 0\n    let v = 7 % zero"),
        (
            "P0006",
            "fn double(x: Int) -> Int\n    requires x > 0\n{\n    x * 2\n}",
            "    let y = double(-1)",
        ),
        (
            "P0007",
            "fn absolute(x: Int) -> Int\n    ensures |r| r >= 0\n{\n    x\n}",
            "    let y = absolute(-1)",
        ),
        ("P0020", "", "    let key = env.secret(\"OSKA_NOT_SET\")"),
        (
            "P0021",
            "fn down(n: Int) -> Int {\n    1 + down(n + 1)\n}",
            "    let d = down(0)",
        ),
        ("P0035", "", "    let s = \"héllo\".slice(2, 9)"),
        ("P0036", "", "    let s = \"ab\".repeat(-1)"),
        ("P0037", "", "    let s = \"ab\".repeat(200000000)"),
    ];
    for (code, items, body) in cases {
        let src = format!(
            "{items}\n\nfn main(io: Stdout, env: Env) {{\n    \
             let _ = io.println(\"before\")\n{body}\n}}\n"
        );
        let outcome = compiled_same(&Case::file(&format!("panic_{code}"), &src));
        assert_eq!(outcome.stdout, "before\n", "{code}: {outcome:#?}");
        assert!(
            outcome.stderr.starts_with("main.osk:")
                && outcome.stderr.contains(&format!(": error[{code}]: "))
                && outcome.stderr.ends_with(")\n")
                && outcome.stderr.lines().count() == 1,
            "{code}: {outcome:#?}"
        );
        assert_eq!(outcome.exit_code, Some(101), "{code}: {outcome:#?}");
    }
}
