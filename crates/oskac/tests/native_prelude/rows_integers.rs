// SPDX-License-Identifier: Apache-2.0
//! Rows of [`CALLS`](super::CALLS): `Scope`, `Task`, the debug helpers and the integer and `Bool` conversions.

use super::{Row, Run};

pub const ROWS: &[Row] = &[
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
];
