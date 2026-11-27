// SPDX-License-Identifier: Apache-2.0
//! Every body-less prelude and `std.html` function has a native
//! implementation (spec #125, #140).
//!
//! [`CALLS`] holds, for each body-less function, by the name the checked
//! program calls it by, a program that calls it. The tests require a call
//! for every one but the [`EXEMPT`] (no backend has a runtime for them),
//! that the program really calls that function, that the native backend
//! compiles it for every target, and that it runs the same on both
//! backends. A new
//! body-less function fails here until it has a native implementation and
//! a row.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use std::ops::ControlFlow;

use differential::{assert_same, in_parallel, Case};
use oska_check::program::{walk, Callee, Program, Visit};
use oska_syntax::ast::{Item, ItemKind, Module, TypeKind};

/// The prelude's source, the same file the checker embeds.
const PRELUDE: &str = include_str!("../../oska-check/src/prelude.osk");

/// `std.html`'s source.
const STD_HTML: &str = include_str!("../../oska-check/src/std/html.osk");

/// Why the native test host does not record network attempts yet.
const NATIVE_NET_ATTEMPTS: &str =
    "a native plain test refuses `t.net` as the interpreter does, but \
does not record where it was asked to go: `t.net_attempts` is a later ticket of spec #228";

/// The `List` methods whose bodies are Oska in `std.core` (ADR 0034): the
/// checker resolves a call to a function of the program, so there is no
/// intrinsic to implement. `oska-run`'s `list_methods` test runs each on
/// both backends.
const CORE_LIST: &str = "written in Oska in std.core, not an intrinsic";

/// The types, or `Type.method` names, that no backend implements, with
/// why. A program given a type that has no runtime is refused before it
/// runs, by the interpreter (`E0370`) and the native backend alike.
/// `TestContext.net_attempts` has a runtime on the interpreter only.
/// `std.sql` and `std.proc` have no runtime on either backend either: a
/// program importing one is refused (`E0371`), so their body-less
/// functions are not scanned at all.
const EXEMPT: &[(&str, &str)] = &[
    ("TestContext.net_attempts", NATIVE_NET_ATTEMPTS),
    ("Spawn", "process spawning has no runtime yet"),
    ("Sched", "scheduling has no runtime yet"),
    ("List.filter", CORE_LIST),
    ("List.fold", CORE_LIST),
    ("List.any", CORE_LIST),
    ("List.all", CORE_LIST),
    ("List.find", CORE_LIST),
    ("List.contains", CORE_LIST),
    ("List.first", CORE_LIST),
    ("List.last", CORE_LIST),
    ("List.is_empty", CORE_LIST),
    ("List.reverse", CORE_LIST),
    ("List.concat", CORE_LIST),
];

/// How a [`CALLS`] program is run.
#[derive(Clone, Copy)]
enum Run {
    /// Its `main`, with nothing on stdin.
    Main,
    /// Its `main`, with this on stdin and in `$NAME`.
    WithInput(&'static str),
    /// Its `test` blocks: only `TestContext` methods.
    Tests,
}

/// For each body-less function, a program that calls it, and how to run
/// it. `main` prints what each call gives, so the differential run checks
/// the value too.
const CALLS: &[(&str, Run, &str)] = &[
    (
        "Stdin.read_line",
        Run::WithInput("a line\n"),
        r#"fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let line = input.read_line()?
    io.println(line.unwrap_or("none"))
}"#,
    ),
    (
        "Stdout.println",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("out")
}"#,
    ),
    (
        "Stderr.println",
        Run::Main,
        r#"fn main(err: Stderr) -> Result<(), IoError> {
    err.println("err")
}"#,
    ),
    (
        "Fs.restrict_to",
        Run::Main,
        r#"fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let here = fs.restrict_to(".")
    here.write_only().write(Path.new("a.txt"), "a")?
    io.println(here.read_only().read_to_string(Path.new("a.txt"))?)
}"#,
    ),
    (
        "Fs.read_only",
        Run::Main,
        r#"fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let names = fs.read_only().restrict_to(".").list_dir(Path.new("."))?
    io.println(",".join(names))
}"#,
    ),
    (
        "Fs.write_only",
        Run::Main,
        r#"fn main(fs: Fs) -> Result<(), IoError> {
    fs.write_only().restrict_to(".").write(Path.new("w.txt"), "w")
}"#,
    ),
    (
        "FsRead.restrict_to",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let names = read.restrict_to(".").list_dir(Path.new("."))?
    io.println(",".join(names))
}"#,
    ),
    (
        "FsRead.restrict_to_file",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    io.println(read.restrict_to_file("main.osk").read_to_string()?.slice(0, 7))
}"#,
    ),
    (
        "FsWrite.restrict_to",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").create_dir(Path.new("made"))
}"#,
    ),
    (
        "FsWrite.restrict_to_file",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to_file("f.txt").write("f")
}"#,
    ),
    (
        "Dir.restrict_to",
        Run::Main,
        r#"fn main(fs: Fs) -> Result<(), IoError> {
    let here = fs.restrict_to(".")
    here.write_only().create_dir(Path.new("sub"))?
    here.restrict_to(Path.new("sub")).write_only().write(Path.new("x.txt"), "x")
}"#,
    ),
    (
        "Dir.read_only",
        Run::Main,
        r#"fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let names = fs.restrict_to(".").read_only().list_dir(Path.new("."))?
    io.println(",".join(names))
}"#,
    ),
    (
        "Dir.write_only",
        Run::Main,
        r#"fn main(fs: Fs) -> Result<(), IoError> {
    fs.restrict_to(".").write_only().write(Path.new("d.txt"), "d")
}"#,
    ),
    (
        "DirRead.restrict_to",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to(".").restrict_to(Path.new("missing")).list_dir(Path.new(".")) {
        Ok(_) => io.println("listed")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "DirRead.restrict_to_file",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let text = read.restrict_to(".").restrict_to_file(Path.new("main.osk")).read_to_string()?
    io.println(text.slice(0, 7))
}"#,
    ),
    (
        "DirRead.read_to_string",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let text = read.restrict_to(".").read_to_string(Path.new("main.osk"))?
    io.println(text.slice(0, 7))
}"#,
    ),
    (
        "DirRead.list_dir",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let names = read.restrict_to(".").list_dir(Path.new("."))?
    io.println(",".join(names))
}"#,
    ),
    (
        "DirWrite.restrict_to",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    let here = write.restrict_to(".")
    here.create_dir(Path.new("sub"))?
    here.restrict_to(Path.new("sub")).write(Path.new("y.txt"), "y")
}"#,
    ),
    (
        "DirWrite.restrict_to_file",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").restrict_to_file(Path.new("z.txt")).write("z")
}"#,
    ),
    (
        "DirWrite.write",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").write(Path.new("out.txt"), "written")
}"#,
    ),
    (
        "DirWrite.create_dir",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").create_dir(Path.new("new"))
}"#,
    ),
    (
        "FileRead.read_to_string",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to_file("missing.txt").read_to_string() {
        Ok(text) => io.println(text)
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "FileWrite.write",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to_file("file.txt").write("file")
}"#,
    ),
    (
        "DirRead.read_bytes",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let bytes = read.restrict_to(".").read_bytes(Path.new("main.osk"))?
    io.println(bytes.slice(0, 7).to_hex())
}"#,
    ),
    (
        "FileRead.read_bytes",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to_file("missing.bin").read_bytes() {
        Ok(bytes) => io.println(bytes.to_hex())
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "DirWrite.write_bytes",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").write_bytes(Path.new("out.bin"), Bytes.zeroed(3))
}"#,
    ),
    (
        "FileWrite.write_bytes",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to_file("file.bin").write_bytes(Bytes.from_hex("80ff").unwrap_or(Bytes.new()))
}"#,
    ),
    (
        "Env.get",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    io.println(env.get("NAME").unwrap_or("unset"))?
    io.println(env.get("UNSET").unwrap_or("unset"))
}"#,
    ),
    (
        "Env.secret",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    let secret = env.secret("NAME")
    let n = secret.expose().len()
    io.println("{n}")
}"#,
    ),
    (
        "Secret.expose",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    io.println(env.secret("NAME").expose())
}"#,
    ),
    (
        "Secret.constant_time_eq",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    let same = env.secret("NAME").constant_time_eq("from env")
    let other = env.secret("NAME").constant_time_eq("other")
    io.println(" ".join([shown(same), shown(other)]))
}"#,
    ),
    (
        "Secret.constant_time_eq_secret",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    let same = env.secret("NAME").constant_time_eq_secret(env.secret("NAME"))
    io.println(shown(same))
}"#,
    ),
    (
        "Secret.to_bytes",
        Run::WithInput(""),
        r#"fn main(io: Stdout, env: Env) -> Result<(), IoError> {
    let key = env.secret("NAME").to_bytes()
    io.println(key.len().to_string())?
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.new",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(k.expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.rotl",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(shown(k.rotl(1).expose() == 3))
}"#,
    ),
    (
        "Secret.rotr",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(shown(k.rotr(1).expose() == 3221225472))
}"#,
    ),
    (
        "Secret.ct_eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(shown(k.ct_eq(Secret.new(3)).expose()))
}"#,
    ),
    (
        "Secret.to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U8> = Secret.new(200)
    io.println(k.to_u32().expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.to_u64",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(k.to_u64().expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.wrapping_to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    io.println(k.wrapping_to_u8().expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.wrapping_to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U64> = Secret.new(4294967297)
    io.println(k.wrapping_to_u32().expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.and",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(shown(Secret.new(true).and(Secret.new(false)).expose()))
}"#,
    ),
    (
        "Secret.or",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(shown(Secret.new(true).or(Secret.new(false)).expose()))
}"#,
    ),
    (
        "Secret.xor",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(shown(Secret.new(true).xor(Secret.new(true)).expose()))
}"#,
    ),
    (
        "Secret.not",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(shown(Secret.new(true).not().expose()))
}"#,
    ),
    (
        "Secret.ct_select",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k: Secret<U32> = Secret.new(2147483649)
    let picked = Secret.new(false).ct_select(k, Secret.new(7))
    io.println(picked.expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.len",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.len().to_string())
}"#,
    ),
    (
        "Secret.at",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.at(7).expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.slice",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.slice(1, 3).expose().to_hex())
}"#,
    ),
    (
        "Secret.read_u32_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.read_u32_be(0).expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.read_u32_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.read_u32_le(0).expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.read_u64_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.read_u64_be(0).expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.read_u64_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    io.println(key.read_u64_le(0).expose().to_int().to_string())
}"#,
    ),
    (
        "Secret.push",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    key.push(Secret.new(9))
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.append",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    key.append(Secret.new(Bytes.zeroed(1)))
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.push_u32_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    let k: Secret<U32> = Secret.new(2147483649)
    key.push_u32_be(k)
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.push_u32_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    let k: Secret<U32> = Secret.new(2147483649)
    key.push_u32_le(k)
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.push_u64_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    let k: Secret<U32> = Secret.new(2147483649)
    key.push_u64_be(k.to_u64())
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Secret.push_u64_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var key = Secret.new(Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new()))
    let k: Secret<U32> = Secret.new(2147483649)
    key.push_u64_le(k.to_u64())
    io.println(key.expose().to_hex())
}"#,
    ),
    (
        "Clock.now_millis",
        Run::Main,
        r#"fn main(io: Stdout, clock: Clock) -> Result<(), IoError> {
    let after_2020 = clock.now_millis() > 1577836800000
    io.println(shown(after_2020))
}"#,
    ),
    (
        "Budget.limit",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    let small = budget.limit(100, 2)
    io.println(shown(small.remaining().calls == Some(2)))
}"#,
    ),
    (
        "Budget.remaining",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    io.println(shown(budget.remaining().tokens.is_some()))
}"#,
    ),
    (
        "Budget.spent",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    let spent = budget.spent()
    let tokens = spent.tokens
    let calls = spent.calls
    io.println("{tokens} {calls}")
}"#,
    ),
    (
        "Budget.reserve",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    io.println(shown(budget.limit(10, 1).reserve(20).is_ok()))
}"#,
    ),
    (
        "Reservation.debit",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    match budget.reserve(20) {
        Ok(r) => r.debit(30, 0, 0, 0)
        Err(_) => {}
    }
    io.println(shown(budget.spent().tokens == 30))
}"#,
    ),
    (
        "Reservation.release",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    let small = budget.limit(10, 1)
    match small.reserve(10) {
        Ok(r) => r.release()
        Err(_) => {}
    }
    io.println(shown(small.remaining().calls == Some(1)))
}"#,
    ),
    (
        "OverBudget.to_string",
        Run::Main,
        r#"fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {
    match budget.limit(10, 1).reserve(20) {
        Ok(_) => io.println("reserved")
        Err(e) => io.println(e.to_string())
    }
}"#,
    ),
    (
        "Random.next_int",
        Run::Main,
        r#"fn main(io: Stdout, random: Random) -> Result<(), IoError> {
    let a = random.next_int()
    let b = random.next_int()
    let differ = a != b
    io.println(shown(differ))
}"#,
    ),
    (
        "Random.secure_bytes",
        Run::Main,
        r#"fn main(io: Stdout, random: Random) -> Result<(), IoError> {
    let a = random.secure_bytes(16)
    let b = random.secure_bytes(16)
    let right_length = a.len() == 16
    let differ = !a.constant_time_eq_secret(b)
    let _ = io.println(shown(right_length))?
    io.println(shown(differ))
}"#,
    ),
    (
        "TestContext.assert",
        Run::Tests,
        r#"test "assert" {
    t.assert(1 < 2)
}"#,
    ),
    (
        "TestContext.assert_eq",
        Run::Tests,
        r#"test "assert_eq" {
    t.assert_eq(1 + 1, 2)
}"#,
    ),
    (
        "TestContext.assert_ne",
        Run::Tests,
        r#"test "assert_ne" {
    t.assert_ne("a", "b")
}"#,
    ),
    (
        "TestContext.fail",
        Run::Tests,
        r#"test "fail" {
    t.fail("on purpose")
}"#,
    ),
    (
        "TestContext.assert_snapshot",
        Run::Tests,
        r#"test "assert_snapshot" {
    t.assert_snapshot([1, 2])
}"#,
    ),
    (
        "TestContext.printed",
        Run::Tests,
        r#"test "printed" {
    let _ = t.stdout.println("seen")
    t.assert_eq(t.printed(), ["seen"])
}"#,
    ),
    (
        "TestContext.answer_approval",
        Run::Tests,
        r#"test "answer_approval" {
    t.answer_approval(true)
    t.assert(t.approver.confirm("send", []).is_some())
    t.assert(!t.approver.confirm("send", []).is_some())
}"#,
    ),
    (
        "TestContext.approvals_asked",
        Run::Tests,
        r#"test "approvals_asked" {
    let _ = t.approver.confirm("send", ["to: a"])
    t.assert_eq(t.approvals_asked().len(), 3)
}"#,
    ),
    (
        "Approver.terminal",
        Run::WithInput("n\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    let approver = Approver.terminal(stdin, stderr)
    io.println(shown(approver.confirm("send", []).is_some()))
}"#,
    ),
    (
        "Approver.confirm",
        Run::WithInput("yes\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    let approver = Approver.terminal(stdin, stderr)
    io.println(shown(approver.confirm("send", ["to: a"]).is_some()))
}"#,
    ),
    (
        "Approval.summary",
        Run::WithInput("y\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    match Approver.terminal(stdin, stderr).confirm("send", []) {
        Some(a) => io.println(a.summary())
        None => io.println("refused")
    }
}"#,
    ),
    (
        "Approval.details",
        Run::WithInput("y\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    match Approver.terminal(stdin, stderr).confirm("send", ["a", "b"]) {
        Some(a) => io.println(",".join(a.details()))
        None => io.println("refused")
    }
}"#,
    ),
    (
        "Approval.matches",
        Run::WithInput("y\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    match Approver.terminal(stdin, stderr).confirm("send", ["a"]) {
        Some(a) => io.println(shown(a.matches("send", ["a"]) && !a.matches("send", ["b"])))
        None => io.println("refused")
    }
}"#,
    ),
    (
        "Approval.consume",
        Run::WithInput("y\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    match Approver.terminal(stdin, stderr).confirm("send", ["a"]) {
        Some(a) => {
            io.println(shown(a.consume("send", ["a"]).is_ok()))?
            io.println(shown(a.consume("send", ["a"]).is_ok()))
        }
        None => io.println("refused")
    }
}"#,
    ),
    (
        "ApprovalError.to_string",
        Run::WithInput("y\n"),
        r#"fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {
    match Approver.terminal(stdin, stderr).confirm("send", ["a"]) {
        Some(a) => match a.consume("send", ["b"]) {
            Ok(_) => io.println("consumed")
            Err(e) => io.println(e.to_string())
        }
        None => io.println("refused")
    }
}"#,
    ),
    (
        "Option.ok_or",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let found: Option<String> = None
    let r = found.ok_or("missing")
    io.println(r.unwrap_or("fallback"))
}"#,
    ),
    (
        "Option.is_some",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let some = Some(1).is_some()
    io.println(shown(some))
}"#,
    ),
    (
        "Option.unwrap_or",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = Some(4).unwrap_or(0)
    io.println("{n}")
}"#,
    ),
    (
        "Result.is_ok",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let r: Result<Int, String> = Err("no")
    let ok = r.is_ok()
    io.println(shown(ok))
}"#,
    ),
    (
        "Result.unwrap_or",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let r: Result<Int, String> = Ok(3)
    let n = r.unwrap_or(0)
    io.println("{n}")
}"#,
    ),
    (
        "List.new",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let xs: List<Int> = List.new()
    let n = xs.len()
    io.println("{n}")
}"#,
    ),
    (
        "List.push",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var xs = [1]
    xs.push(2)
    let n = xs.len()
    io.println("{n}")
}"#,
    ),
    (
        "List.len",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = [1, 2, 3].len()
    io.println("{n}")
}"#,
    ),
    (
        "List.count",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = ["a", "b"].count()
    io.println("{n}")
}"#,
    ),
    (
        "List.get",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let xs = ["a", "b"]
    io.println(xs.get(1).unwrap_or("none"))?
    io.println(xs.get(5).unwrap_or("none"))
}"#,
    ),
    (
        "List.at",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let xs = ["a", "b"]
    io.println(xs.at(0))?
    io.println(xs.at(2))
}"#,
    ),
    (
        "List.map",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let xs = [1, 2, 3].map(|x| "<{x}>")
    io.println("".join(xs))
}"#,
    ),
    (
        "String.lines",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = "a\nb\r\nc".lines().len()
    io.println("{n}")
}"#,
    ),
    (
        "String.len",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = "héllo".len()
    io.println("{n}")
}"#,
    ),
    (
        "String.byte_len",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = "héllo".byte_len()
    io.println("{n}")
}"#,
    ),
    (
        "String.trim",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = "  x  ".trim()
    io.println("[{x}]")
}"#,
    ),
    (
        "String.split",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("|".join("a,b,,c".split(",")))
}"#,
    ),
    (
        "String.contains",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let yes = "haystack".contains("st")
    io.println(shown(yes))
}"#,
    ),
    (
        "String.starts_with",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let yes = "haystack".starts_with("hay")
    io.println(shown(yes))
}"#,
    ),
    (
        "String.ends_with",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let no = "haystack".ends_with("hay")
    io.println(shown(no))
}"#,
    ),
    (
        "String.replace",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("a-b-c".replace("-", "+"))
}"#,
    ),
    (
        "String.join",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(", ".join(["x", "y"]))
}"#,
    ),
    (
        "String.chars",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("/".join("añb".chars()))
}"#,
    ),
    (
        "String.slice",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hello".slice(1, 3))?
    io.println("hello".slice(3, 1))
}"#,
    ),
    (
        "String.repeat",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("ab".repeat(3))?
    io.println("ab".repeat(-1))
}"#,
    ),
    (
        "String.to_lowercase",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("ÀB Σ".to_lowercase())
}"#,
    ),
    (
        "String.parse_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = "-42".parse_int().unwrap_or(0)
    let bad = shown("4x".parse_int().is_some())
    io.println("{n} {bad}")
}"#,
    ),
    (
        "Bytes.assume_trusted",
        Run::WithInput("typed\n"),
        r#"fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let line = input.read_line()?.unwrap_or("")
    let trusted: Bytes = line.to_bytes().assume_trusted()
    io.println(trusted.to_hex())
}"#,
    ),
    (
        "String.assume_trusted",
        Run::WithInput("typed\n"),
        r#"fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let line = input.read_line()?.unwrap_or("")
    let trusted: String = line.assume_trusted()
    io.println(trusted)
}"#,
    ),
    (
        "Path.new",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let same = Path.new("a") == Path.new("a")
    io.println(shown(same))
}"#,
    ),
    (
        "Path.from",
        Run::WithInput("b\n"),
        r#"fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let line = input.read_line()?.unwrap_or("")
    let same = Path.from(line) == Path.new("b")
    io.println(shown(same))
}"#,
    ),
    (
        "Args.get",
        Run::Main,
        r#"fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    io.println(args.get(1).unwrap_or("no argument"))
}"#,
    ),
    (
        "IoError.to_string",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to("missing").list_dir(Path.new(".")) {
        Ok(_) => io.println("listed")
        Err(e) => io.println(e.to_string())
    }
}"#,
    ),
    (
        "Scope.spawn",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let total = scope |s| {
        let a = s.spawn(|| 20)
        a.join() + 1
    }
    io.println("{total}")
}"#,
    ),
    (
        "Task.join",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let text = scope |s| {
        let a = s.spawn(|| "joined")
        a.join()
    }
    io.println(text)
}"#,
    ),
    (
        "dbg",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = dbg(1 + 2)
    let _ = dbg(Some("x"))
    io.println("{n}")
}"#,
    ),
    (
        "todo",
        Run::Main,
        r#"fn later() -> Int {
    todo()
}

fn main(io: Stdout) -> Result<(), IoError> {
    let n = later()
    io.println("{n}")
}"#,
    ),
    (
        "map_entry_limit",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("before")?
    map_entry_limit()
    io.println("after")
}"#,
    ),
    (
        "map_probe_limit",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("before")?
    map_probe_limit()
    io.println("after")
}"#,
    ),
    (
        "Int.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = 3.eq(3)
    io.println(shown(a))
}"#,
    ),
    (
        "I8.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I8 = -3
    let a = x.eq(-3)
    io.println(shown(a))
}"#,
    ),
    (
        "I16.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I16 = 300
    let a = x.eq(301)
    io.println(shown(a))
}"#,
    ),
    (
        "I32.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I32 = 70000
    let a = x.eq(70000)
    io.println(shown(a))
}"#,
    ),
    (
        "I64.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I64 = 5
    let a = x.eq(5)
    io.println(shown(a))
}"#,
    ),
    (
        "U8.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 255
    let a = x.eq(255)
    io.println(shown(a))
}"#,
    ),
    (
        "U16.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 65535
    let a = x.eq(0)
    io.println(shown(a))
}"#,
    ),
    (
        "U32.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4000000000
    let a = x.eq(4000000000)
    io.println(shown(a))
}"#,
    ),
    (
        "U64.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551615
    let a = x.eq(18446744073709551615)
    io.println(shown(a))
}"#,
    ),
    (
        "Int.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = 3.cmp(4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "I8.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I8 = -3
    let a = x.cmp(-4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "I16.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I16 = -3
    let a = x.cmp(-4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "I32.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I32 = -3
    let a = x.cmp(-4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "I64.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I64 = -3
    let a = x.cmp(-4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "U8.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 3
    let a = x.cmp(4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "U16.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 3
    let a = x.cmp(4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "U32.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 3
    let a = x.cmp(4)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "U64.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551615
    let a = x.cmp(1)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "Bool.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = false.cmp(true)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "String.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = "b".cmp("ab")
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "Option.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = Some("b").cmp(None)
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "List.cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = [[1, 2], [3]].cmp([[1, 2, 0]])
    let word = match a {
        Ordering.Less => "less"
        Ordering.Equal => "equal"
        Ordering.Greater => "greater"
    }
    io.println(word)
}"#,
    ),
    (
        "Bool.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = true.eq(false)
    io.println(shown(a))
}"#,
    ),
    (
        "String.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = "x".eq("x")
    io.println(shown(a))
}"#,
    ),
    (
        "Path.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = Path.new("p").eq(Path.new("q"))
    io.println(shown(a))
}"#,
    ),
    (
        "IoError.eq",
        Run::Main,
        r#"fn refusal(read: FsRead) -> IoError {
    match read.restrict_to("gone").list_dir(Path.new(".")) {
        Ok(_) => IoError(message: "listed")
        Err(e) => e
    }
}

fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let same = refusal(read).eq(refusal(read))
    io.println(shown(same))
}"#,
    ),
    (
        "Option.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = Some(1).eq(Some(1))
    let b = Some("x") == None
    io.println(" ".join([shown(a), shown(b)]))
}"#,
    ),
    (
        "Result.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let r: Result<Int, String> = Ok(1)
    let a = r.eq(Ok(1))
    io.println(shown(a))
}"#,
    ),
    (
        "List.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = [1, 2].eq([1, 2])
    let b = ["a"] == ["b"]
    io.println(" ".join([shown(a), shown(b)]))
}"#,
    ),
    (
        "String.to_string",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("text".to_string())
}"#,
    ),
    (
        "U8.rotl",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 129
    io.println(shown(x.rotl(1) == 3))
}"#,
    ),
    (
        "U8.rotr",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 129
    io.println(shown(x.rotr(1) == 192))
}"#,
    ),
    (
        "U32.rotl",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 2147483649
    io.println(shown(x.rotl(1) == 3))
}"#,
    ),
    (
        "U32.rotr",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 2147483649
    io.println(shown(x.rotr(1) == 3221225472))
}"#,
    ),
    (
        "U64.rotl",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 9223372036854775809
    io.println(shown(x.rotl(1) == 3))
}"#,
    ),
    (
        "U64.rotr",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 9223372036854775809
    io.println(shown(x.rotr(1) == 13835058055282163712))
}"#,
    ),
    (
        "U8.wrapping_add",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 255
    io.println(shown(x.wrapping_add(2) == 1))
}"#,
    ),
    (
        "U8.wrapping_sub",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 1
    io.println(shown(x.wrapping_sub(2) == 255))
}"#,
    ),
    (
        "U8.wrapping_mul",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 255
    io.println(shown(x.wrapping_mul(3) == 253))
}"#,
    ),
    (
        "U32.wrapping_add",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4294967295
    io.println(shown(x.wrapping_add(2) == 1))
}"#,
    ),
    (
        "U32.wrapping_sub",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 1
    io.println(shown(x.wrapping_sub(2) == 4294967295))
}"#,
    ),
    (
        "U32.wrapping_mul",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4294967295
    io.println(shown(x.wrapping_mul(3) == 4294967293))
}"#,
    ),
    (
        "U64.wrapping_add",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551615
    io.println(shown(x.wrapping_add(2) == 1))
}"#,
    ),
    (
        "U64.wrapping_sub",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 1
    io.println(shown(x.wrapping_sub(2) == 18446744073709551615))
}"#,
    ),
    (
        "U64.wrapping_mul",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551615
    io.println(shown(x.wrapping_mul(3) == 18446744073709551613))
}"#,
    ),
    (
        "Float.sqrt",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 2.25
    io.println(shown(x.sqrt() == 1.5))
}"#,
    ),
    (
        "Float.to_bits",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 1.0
    io.println(shown(x.to_bits() == 4607182418800017408))
}"#,
    ),
    (
        "Float.from_bits",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 4609434218613702656
    io.println(shown(Float.from_bits(x) == 1.5))
}"#,
    ),
    (
        "Float.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let zero = 0.0
    let a = zero.eq(-zero)
    io.println(shown(a))
}"#,
    ),
    (
        "Float.to_string",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 0.1
    io.println(x.to_string())
}"#,
    ),
    (
        "Float.to_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 2.9
    let n = x.to_int()
    io.println(n.to_string())
}"#,
    ),
    (
        "Float.fmt",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 2.675
    io.println(x.fmt(2))
}"#,
    ),
    (
        "Float.total_cmp",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 1.0
    let order = x.total_cmp(2.0)
    io.println(order.to_string())
}"#,
    ),
    (
        "Float.approx_eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 1.0
    io.println(shown(x.approx_eq(1.0000001, 1e-6, 0.0)))
}"#,
    ),
    (
        "Int.to_float",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = 3
    let x = n.to_float()
    io.println(x.to_string())
}"#,
    ),
    (
        "Int.to_float_rounded",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let n = 9007199254740993
    let x = n.to_float_rounded()
    io.println(x.to_string())
}"#,
    ),
    (
        "String.parse_float",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = "1.5e3".parse_float().unwrap_or(0.0)
    io.println(x.to_string())
}"#,
    ),
    (
        "String.parse_float_lenient",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = "+inf".parse_float_lenient().unwrap_or(0.0)
    io.println(x.to_string())
}"#,
    ),
    (
        "Int.to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: Int = 200
    io.println(shown(x.to_u8() == 200))
}"#,
    ),
    (
        "Int.to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: Int = 200
    io.println(shown(x.to_u16() == 200))
}"#,
    ),
    (
        "Int.to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: Int = 200
    io.println(shown(x.to_u32() == 200))
}"#,
    ),
    (
        "Int.to_u64",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: Int = 200
    io.println(shown(x.to_u64() == 200))
}"#,
    ),
    (
        "U8.to_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 200
    io.println(shown(x.to_int() == 200))
}"#,
    ),
    (
        "U8.to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 200
    io.println(shown(x.to_u16() == 200))
}"#,
    ),
    (
        "U8.to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 200
    io.println(shown(x.to_u32() == 200))
}"#,
    ),
    (
        "U8.to_u64",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 200
    io.println(shown(x.to_u64() == 200))
}"#,
    ),
    (
        "U16.to_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 200
    io.println(shown(x.to_int() == 200))
}"#,
    ),
    (
        "U16.to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 200
    io.println(shown(x.to_u8() == 200))
}"#,
    ),
    (
        "U16.to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 200
    io.println(shown(x.to_u32() == 200))
}"#,
    ),
    (
        "U16.to_u64",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 200
    io.println(shown(x.to_u64() == 200))
}"#,
    ),
    (
        "U16.wrapping_to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 65534
    io.println(shown(x.wrapping_to_u8() == 254))
}"#,
    ),
    (
        "U32.to_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 200
    io.println(shown(x.to_int() == 200))
}"#,
    ),
    (
        "U32.to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 200
    io.println(shown(x.to_u8() == 200))
}"#,
    ),
    (
        "U32.to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 200
    io.println(shown(x.to_u16() == 200))
}"#,
    ),
    (
        "U32.to_u64",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 200
    io.println(shown(x.to_u64() == 200))
}"#,
    ),
    (
        "U32.wrapping_to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4294967294
    io.println(shown(x.wrapping_to_u8() == 254))
}"#,
    ),
    (
        "U32.wrapping_to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4294967294
    io.println(shown(x.wrapping_to_u16() == 65534))
}"#,
    ),
    (
        "U64.to_int",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 200
    io.println(shown(x.to_int() == 200))
}"#,
    ),
    (
        "U64.to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 200
    io.println(shown(x.to_u8() == 200))
}"#,
    ),
    (
        "U64.to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 200
    io.println(shown(x.to_u16() == 200))
}"#,
    ),
    (
        "U64.to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 200
    io.println(shown(x.to_u32() == 200))
}"#,
    ),
    (
        "U64.wrapping_to_u8",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551614
    io.println(shown(x.wrapping_to_u8() == 254))
}"#,
    ),
    (
        "U64.wrapping_to_u16",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551614
    io.println(shown(x.wrapping_to_u16() == 65534))
}"#,
    ),
    (
        "U64.wrapping_to_u32",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551614
    io.println(shown(x.wrapping_to_u32() == 4294967294))
}"#,
    ),
    (
        "Bytes.new",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(Bytes.new().to_hex())
}"#,
    ),
    (
        "Bytes.zeroed",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(Bytes.zeroed(3).to_hex())
}"#,
    ),
    (
        "Bytes.from_hex",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(shown(Bytes.from_hex("0aF0").is_some()))
}"#,
    ),
    (
        "Bytes.len",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    let n = b.len()
    io.println("{n}")
}"#,
    ),
    (
        "Bytes.get",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(shown(b.get(3) == Some(255) && b.get(4) == None))
}"#,
    ),
    (
        "Bytes.at",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.at(2).to_int().to_string())
}"#,
    ),
    (
        "Bytes.slice",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.slice(1, 3).to_hex())
}"#,
    ),
    (
        "Bytes.read_u16_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.read_u16_be(2).to_int().to_string())
}"#,
    ),
    (
        "Bytes.read_u16_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.read_u16_le(2).to_int().to_string())
}"#,
    ),
    (
        "Bytes.read_u32_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.read_u32_be(0).to_int().to_string())
}"#,
    ),
    (
        "Bytes.read_u32_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.read_u32_le(0).to_int().to_string())
}"#,
    ),
    (
        "Bytes.read_u64_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    let c = b.slice(0, 4)
    var d = b
    d.append(c)
    io.println(shown(d.read_u64_be(0) == 72797561085403391))
}"#,
    ),
    (
        "Bytes.read_u64_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var d = b
    d.append(b)
    io.println(shown(d.read_u64_le(0) == 18419724683552227841))
}"#,
    ),
    (
        "Bytes.to_hex",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.to_hex())
}"#,
    ),
    (
        "Bytes.to_text",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println("hé".to_bytes().to_text().unwrap_or("not UTF-8"))
}"#,
    ),
    (
        "Bytes.push",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = b
    c.push(9)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.set",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = b
    c.set(0, 9)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.append",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = b
    c.append(b)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u16_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U16 = 258
    c.push_u16_be(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u16_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U16 = 258
    c.push_u16_le(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u32_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U32 = 16909060
    c.push_u32_be(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u32_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U32 = 16909060
    c.push_u32_le(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u64_be",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U64 = 72623859790382856
    c.push_u64_be(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.push_u64_le",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    var c = Bytes.new()
    let v: U64 = 72623859790382856
    c.push_u64_le(v)
    io.println(c.to_hex())
}"#,
    ),
    (
        "Bytes.eq",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(shown(b == b.slice(0, 4) && b != Bytes.new()))
}"#,
    ),
    (
        "Bytes.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println(b.hash().to_string())
}"#,
    ),
    (
        "Int.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = 7
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "I8.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I8 = -3
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "I16.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I16 = -3
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "I32.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I32 = -3
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "I64.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: I64 = -3
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "U8.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U8 = 200
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "U16.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U16 = 60000
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "U32.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U32 = 4000000000
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "U64.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x: U64 = 18446744073709551615
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "Bool.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let x = true
    io.println(x.hash().to_string())
}"#,
    ),
    (
        "String.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let s = "héllo"
    io.println(s.hash().to_string())
}"#,
    ),
    (
        "Path.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(Path.new("a/b").hash().to_string())
}"#,
    ),
    (
        "Option.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a: Option<String> = Some("x")
    let b: Option<String> = None
    io.println(" ".join([a.hash().to_string(), b.hash().to_string()]))
}"#,
    ),
    (
        "List.hash",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let a = [[1, 2], [3]]
    io.println(a.hash().to_string())
}"#,
    ),
    (
        "String.to_bytes",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let b = Bytes.from_hex("0102a0ff").unwrap_or(Bytes.new())
    io.println("hé".to_bytes().to_hex())
}"#,
    ),
    (
        "Int.to_string",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(42.to_string())
}"#,
    ),
    (
        "Net.connect_only",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let connect = net.connect_only()
    let host = connect.restrict_to("127.0.0.1", 9)
    io.println(host.name())
}"#,
    ),
    (
        "Net.listen_only",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let port = listener.port()
    io.println(if port > 0 { "listening" } else { "not listening" })
}"#,
    ),
    (
        "Net.restrict_to",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let host = net.restrict_to("example.com", 443)
    io.println(host.name())
}"#,
    ),
    (
        "NetConnect.restrict_to",
        Run::Main,
        r#"fn main(io: Stdout, net: NetConnect) -> Result<(), IoError> {
    let host = net.restrict_to("example.org", 8080)
    io.println(host.name())
}"#,
    ),
    (
        "NetListen.restrict_to_loopback",
        Run::Main,
        r#"fn main(io: Stdout, net: NetListen) -> Result<(), IoError> {
    let listener = net.restrict_to_loopback(70000)
    match listener.accept() {
        Ok(_) => io.println("accepted")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "HostConnect.name",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    io.println(net.restrict_to("host.example", 1).name())
}"#,
    ),
    (
        "HostConnect.port",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    io.println(net.restrict_to("host.example", 4321).port().to_string())
}"#,
    ),
    (
        "HostConnect.connect",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    match net.restrict_to("not a host", 1).connect() {
        Ok(_) => io.println("connected")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "HostConnect.exchange_datagram",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    match net.restrict_to("example.com", 53).exchange_datagram("12".to_bytes()) {
        Ok(_) => io.println("answered")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "NetConnect.system_name_servers",
        Run::Main,
        r#"fn main(io: Stdout, net: NetConnect) -> Result<(), IoError> {
    let servers = net.system_name_servers()?
    io.println(if servers.len() <= 16 { "listed" } else { "too many" })
}"#,
    ),
    (
        "LoopbackListen.accept",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    io.println(server.host())
}"#,
    ),
    (
        "LoopbackListen.port",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    io.println(if listener.port() > 0 { "a port" } else { "no port" })
}"#,
    ),
    (
        "Connection.send",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    client.send("sent")?
    io.println(server.receive()?)
}"#,
    ),
    (
        "Connection.receive",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    server.send("received")?
    io.println(client.receive()?)
}"#,
    ),
    (
        "Connection.send_bytes",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    client.send_bytes("bytes".to_bytes())?
    io.println(server.receive()?)
}"#,
    ),
    (
        "Connection.receive_bytes",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    server.send("hex")?
    io.println(client.receive_bytes()?.assume_trusted().to_hex())
}"#,
    ),
    (
        "Connection.host",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("localhost", listener.port()).connect()?
    io.println(client.host())
}"#,
    ),
    (
        "Connection.close",
        Run::Main,
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    client.close()
    io.println(server.receive()?)
}"#,
    ),
    (
        "text",
        Run::Main,
        r#"use std.html

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(html.text("<b>&'\"").render())
}"#,
    ),
    (
        "TrustedHtml.new",
        Run::Main,
        r#"use std.html

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(html.Html.raw(html.TrustedHtml.new("<p>")).render())
}"#,
    ),
    (
        "Html.raw",
        Run::Main,
        r#"use std.html

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(html.Html.raw(html.TrustedHtml.new("<br>")).render())
}"#,
    ),
    (
        "Html.concat",
        Run::Main,
        r#"use std.html

fn main(io: Stdout) -> Result<(), IoError> {
    let page = html.Html.raw(html.TrustedHtml.new("<p>")).concat(html.text("a<b"))
    io.println(page.render())
}"#,
    ),
    (
        "Html.render",
        Run::Main,
        r#"use std.html

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(html.text("x").render())
}"#,
    ),
];

/// The qualified name the checked program calls `item`'s function by, if
/// it has no body.
fn bodiless(owner: Option<&str>, item: &Item) -> Option<String> {
    let ItemKind::Fn(f) = &item.kind else {
        return None;
    };
    if f.body.is_some() {
        return None;
    }
    Some(match owner {
        Some(owner) => format!("{owner}.{}", f.name.name),
        None => f.name.name.clone(),
    })
}

/// The type name methods on `target` are called by: trust is
/// transparent, so `Untrusted<String>`'s are `String`'s.
fn call_owner(target: &oska_syntax::ast::Type) -> Option<String> {
    let TypeKind::Path { segments, args } = &target.kind else {
        return None;
    };
    let head = &segments.last()?.name;
    match (head.as_str(), args.as_slice()) {
        ("Untrusted", [inner]) => call_owner(inner),
        _ => Some(head.clone()),
    }
}

/// Every body-less function `module` declares, by qualified name.
fn bodiless_functions(module: &Module) -> Vec<String> {
    let mut names = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(_) => names.extend(bodiless(None, item)),
            ItemKind::Impl(decl) => {
                let Some(owner) = call_owner(&decl.target) else {
                    continue;
                };
                for method in &decl.methods {
                    names.extend(bodiless(Some(&owner), method));
                }
            }
            _ => {}
        }
    }
    names
}

/// Every body-less function of the prelude and `std.html` that a backend
/// must implement.
fn required() -> Vec<String> {
    let mut names = Vec::new();
    for src in [PRELUDE, STD_HTML] {
        let module = oska_syntax::parse_prelude(src).expect("the source should parse");
        names.extend(bodiless_functions(&module));
    }
    names.retain(|name| {
        let owner = name.split('.').next().unwrap_or("");
        !EXEMPT
            .iter()
            .any(|(e, _)| *e == owner || *e == name.as_str())
    });
    names
}

#[test]
fn every_bodiless_function_has_a_call() {
    let names = required();
    assert!(
        names.len() > 60 && names.iter().any(|n| n == "Stdout.println"),
        "the scan should find the prelude's functions: {names:?}"
    );
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !CALLS.iter().any(|(name, _, _)| name == n))
        .collect();
    assert!(
        missing.is_empty(),
        "body-less functions with no call here, so no proof of a native implementation: {missing:?}"
    );
    for (name, _, _) in CALLS {
        assert!(names.iter().any(|n| n == name), "`{name}` is not body-less");
    }
}

/// The whole program of a [`CALLS`] row: its source and `shown`, which
/// words a `Bool` for printing (`Bool` is not `Display`).
fn source(src: &str) -> String {
    format!("{src}\n\nfn shown(b: Bool) -> String {{\n    if b {{ \"true\" }} else {{ \"false\" }}\n}}\n")
}

/// The checked program of a [`CALLS`] row, which must check.
fn checked(name: &str, src: &str) -> Program {
    let report = oska_check::check_module(src);
    match report.program {
        Some(program) if report.diagnostics.is_empty() => program,
        _ => panic!(
            "the call of `{name}` should check: {:#?}",
            report.diagnostics
        ),
    }
}

/// Whether `program` calls the intrinsic `name` anywhere.
fn calls(program: &Program, name: &str) -> bool {
    let qualified = format!("html.{name}");
    let bodies = program
        .functions
        .values()
        .map(|f| &f.body)
        .chain(program.tests.iter().map(|t| &t.body));
    for body in bodies {
        let found = walk(body, &mut |visit| match visit {
            Visit::Callee(Callee::Intrinsic(callee), _)
                if *callee == name || *callee == qualified =>
            {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        });
        if found.is_break() {
            return true;
        }
    }
    false
}

#[test]
fn every_call_calls_its_function_and_builds_natively() {
    in_parallel(CALLS, |(name, run, src)| {
        let program = checked(name, &source(src));
        assert!(
            calls(&program, name),
            "the program for `{name}` never calls it"
        );
        for target in oska_native::Target::ALL {
            let built = match run {
                Run::Tests => oska_native::build_tests(&program, target),
                Run::Main | Run::WithInput(_) => {
                    oska_native::build(&program, target, &oska_native::Unplaced)
                }
            };
            if let Err(refusal) = built {
                panic!(
                    "`{name}` has no native implementation for {}: {refusal:#?}",
                    target.name()
                );
            }
        }
    });
}

#[test]
fn every_call_runs_the_same_on_both_backends() {
    in_parallel(CALLS, |(name, run, src)| {
        let case = Case::file(&format!("prelude-{name}"), &source(src)).with_args(&["given"]);
        let case = match run {
            Run::Tests => return,
            Run::Main => case,
            Run::WithInput(stdin) => case.with_stdin(stdin).with_env("NAME", "from env"),
        };
        assert_same(&case);
    });
}
