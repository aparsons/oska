//! Every body-less prelude and `std.html` function has a native
//! implementation (spec #125, #140).
//!
//! [`CALLS`] holds, for each body-less function, by the name the checked
//! program calls it by, a program that calls it. The tests require a call
//! for every one but the [`EXEMPT`] (no backend has a runtime for them),
//! that the program really calls that function, that the native backend
//! compiles it, and that it runs the same on both backends. A new
//! body-less function fails here until it has a native implementation and
//! a row.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use std::ops::ControlFlow;

use differential::{assert_same, Case};
use oska_check::program::{walk, Callee, Program, Visit};
use oska_syntax::ast::{Item, ItemKind, Module, TypeKind};

/// The prelude's source, the same file the checker embeds.
const PRELUDE: &str = include_str!("../../oska-check/src/prelude.osk");

/// `std.html`'s source.
const STD_HTML: &str = include_str!("../../oska-check/src/std/html.osk");

/// The types whose methods no backend implements, with why. A program
/// given one is refused before it runs, by the interpreter (`E0370`) and
/// the native backend alike. `std.sql` and `std.proc` have no runtime on
/// either backend either: a program importing one is refused (`E0371`),
/// so their body-less functions are not scanned at all.
const EXEMPT: &[(&str, &str)] = &[
    ("Net", "networking has no runtime yet"),
    ("NetConnect", "networking has no runtime yet"),
    ("NetListen", "networking has no runtime yet"),
    ("Connection", "networking has no runtime yet"),
    ("Spawn", "process spawning has no runtime yet"),
    ("Sched", "scheduling has no runtime yet"),
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
        "Clock.now_millis",
        Run::Main,
        r#"fn main(io: Stdout, clock: Clock) -> Result<(), IoError> {
    let after_2020 = clock.now_millis() > 1577836800000
    io.println(shown(after_2020))
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
        "TestContext.printed",
        Run::Tests,
        r#"test "printed" {
    let _ = t.stdout.println("seen")
    t.assert_eq(t.printed(), ["seen"])
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
        "Int.to_string",
        Run::Main,
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    io.println(42.to_string())
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
        !EXEMPT.iter().any(|(e, _)| *e == owner)
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
    for (name, run, src) in CALLS {
        let program = checked(name, &source(src));
        assert!(
            calls(&program, name),
            "the program for `{name}` never calls it"
        );
        let built = match run {
            Run::Tests => oska_native::build_tests(&program),
            Run::Main | Run::WithInput(_) => oska_native::build(&program, &|d| d),
        };
        if let Err(refusal) = built {
            panic!("`{name}` has no native implementation: {refusal:#?}");
        }
    }
}

#[test]
fn every_call_runs_the_same_on_both_backends() {
    for (name, run, src) in CALLS {
        let case = Case::file(&format!("prelude-{name}"), &source(src)).with_args(&["given"]);
        let case = match run {
            Run::Tests => continue,
            Run::Main => case,
            Run::WithInput(stdin) => case.with_stdin(stdin).with_env("NAME", "from env"),
        };
        assert_same(&case);
    }
}
