// SPDX-License-Identifier: Apache-2.0
//! Rows of [`CALLS`](super::CALLS): Byte conversions, `Float`, integer arithmetic and the remaining rows.

use super::{Row, Run};

pub const ROWS: &[Row] = &[
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
