// SPDX-License-Identifier: Apache-2.0
//! Rows of [`CALLS`](super::CALLS): Stdin, Stdout, Stderr, the file-system capabilities, `Env` and `Secret`.

use super::{Row, Run};

pub const ROWS: &[Row] = &[
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
        "DirRead.exists",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    let there = read.restrict_to(".").exists(Path.new("missing"))?
    io.println(if there { "yes" } else { "no" })
}"#,
    ),
    (
        "DirRead.metadata",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to(".").metadata(Path.new("missing")) {
        Ok(m) => io.println("found")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "DirWrite.append",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to(".").append(Path.new("log.txt"), "line")
}"#,
    ),
    (
        "DirWrite.remove_file",
        Run::Main,
        r#"fn main(io: Stdout, write: FsWrite) -> Result<(), IoError> {
    match write.restrict_to(".").remove_file(Path.new("missing")) {
        Ok(_) => io.println("removed")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "DirWrite.remove_dir",
        Run::Main,
        r#"fn main(io: Stdout, write: FsWrite) -> Result<(), IoError> {
    match write.restrict_to(".").remove_dir(Path.new("missing")) {
        Ok(_) => io.println("removed")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "DirWrite.rename",
        Run::Main,
        r#"fn main(io: Stdout, write: FsWrite) -> Result<(), IoError> {
    match write.restrict_to(".").rename(Path.new("missing"), Path.new("other")) {
        Ok(_) => io.println("moved")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "FileRead.metadata",
        Run::Main,
        r#"fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {
    match read.restrict_to_file("missing.txt").metadata() {
        Ok(m) => io.println("found")
        Err(e) => io.println(e.message)
    }
}"#,
    ),
    (
        "FileWrite.append",
        Run::Main,
        r#"fn main(write: FsWrite) -> Result<(), IoError> {
    write.restrict_to_file("file.txt").append("more")
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
];
