// SPDX-License-Identifier: Apache-2.0
//! Rows of [`CALLS`](super::CALLS): `Clock`, `Budget`, `Random`, `TestContext`, approvals, `Option`, `Result`, `List`, `String`, `Bytes`, `Path` and `IoError`.

use super::{Row, Run};

pub const ROWS: &[Row] = &[
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
        "List.set",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var xs = [1, 2, 3]
    let before = xs
    xs.set(1, 20)
    let a = xs.at(1)
    let b = before.at(1)
    io.println("{a} {b}")
}"#,
    ),
    (
        "List.pop",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var xs = ["a", "b"]
    let last = xs.pop().unwrap_or("none")
    let n = xs.len()
    io.println("{last} {n}")
}"#,
    ),
    (
        "List.truncate",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    var xs = ["a", "b", "c"]
    xs.truncate(1)
    let n = xs.len()
    io.println("{n}")
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
        "String.to_uppercase",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("àb ß".to_uppercase())
}"#,
    ),
    (
        "String.trim_start",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let padded = "  é "
    let cut = padded.trim_start()
    io.println("[{cut}]")
}"#,
    ),
    (
        "String.trim_end",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let padded = "  é "
    let cut = padded.trim_end()
    io.println("[{cut}]")
}"#,
    ),
    (
        "String.find",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let at = "héllo".find("l").unwrap_or(-1)
    io.println("{at}")
}"#,
    ),
    (
        "String.split_once",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let parts = "a=b".split_once("=").unwrap_or([])
    io.println("/".join(parts))
}"#,
    ),
    (
        "String.strip_prefix",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("foobar".strip_prefix("foo").unwrap_or("none"))
}"#,
    ),
    (
        "String.strip_suffix",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println("foobar".strip_suffix("bar").unwrap_or("none"))
}"#,
    ),
    (
        "String.is_empty",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let e = shown("".is_empty())
    io.println(e)
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
];
