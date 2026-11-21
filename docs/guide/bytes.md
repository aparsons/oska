# Bytes and binary data

`Bytes` holds binary data: file contents that are not UTF-8, network records, hashes and keys. This page shows how to make, read and build a buffer, how integers move in and out of it, and how trust follows the bytes. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints. For key material, see [secrets](secrets.md).

## Making a buffer

`Bytes` is a prelude type. There is no byte literal; a buffer starts in one of four ways:

- `Bytes.new()` is empty.
- `Bytes.zeroed(len)` holds `len` zero bytes, to fill in with `set`.
- `Bytes.from_hex(text)` reads two hexadecimal digits a byte, in either case, and gives `None` when the text is not hexadecimal. It is the way to write a test vector.
- `"text".to_bytes()` is the text's UTF-8.

`b.to_hex()` gives lower-case hexadecimal, and `b.to_text()` gives `Option<String>`: `None` when the bytes are not UTF-8. `Bytes` has no `Display`, so print it with `to_hex()`. `dbg` prints it as hex too, as in `Bytes(0a0b0c)`.

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    let greeting = "hé".to_bytes()
    let hex = greeting.to_hex()
    let size = greeting.len()
    io.println("{hex} is {size} bytes")?
    let back = Bytes.from_hex("6869").unwrap_or(Bytes.new())
    let text = back.to_text().unwrap_or("?")
    io.println(text)?
    let invalid = Bytes.from_hex("ff").unwrap_or(Bytes.new())
    match invalid.to_text() {
        Some(_) => io.println("utf-8")?
        None => io.println("not utf-8")?
    }
    Ok(())
}
```

```console cmd=run
68c3a9 is 3 bytes
hi
not utf-8
```

`len()` counts bytes, and is an `Int`, so lengths work with ranges and list indices.

## Reading

Bytes index as a `List` does:

- `b.at(i)` gives the `U8` at `i`, and panics (`P0002`) when `i` is out of bounds. `b.get(i)` gives `Option<U8>` instead.
- `b.slice(start, end)` gives a new `Bytes` from `start` up to, but excluding, `end`. Bounds that are out of range or reversed panic (`P0042`); nothing is cut short silently.
- `read_u16_be`, `read_u32_be` and `read_u64_be` read an integer at a byte offset, most significant byte first (big-endian, the network's order). The `_le` forms read it least significant byte first (little-endian, the order of most file formats). A read whose bytes do not all lie inside the buffer panics (`P0042`).

`==` and `!=` compare two buffers, and `Bytes` is `Hash`.

## Building

A mutating method needs a `var` binding:

- `push(byte)` adds a `U8`, and `append(other)` adds another buffer's bytes.
- `push_u16_be`, `push_u32_be` and `push_u64_be`, and their `_le` forms, add an integer's bytes in that order.
- `set(i, byte)` replaces one byte, and panics (`P0002`) when `i` is out of bounds.

`Bytes` is a value, like every Oska value. A mutating method changes a buffer that nothing else holds in place, and copies a shared one first, so appending in a loop is not quadratic and a copy never changes behind your back. A buffer that would grow past 256 MiB panics (`P0043`). That limit is not yet checked where a whole buffer comes in at once: `String.to_bytes` and the capabilities' `read_bytes` give a buffer of any length.

## Integers in a buffer

The bytes a buffer gives are `U8`, and the endian reads give `U16`, `U32` and `U64`. Integer types convert only explicitly:

- `to_int()`, `to_u8()`, `to_u16()`, `to_u32()` and `to_u64()` convert between `Int`, `U8`, `U16`, `U32` and `U64`. Widening never fails; a value that does not fit its target panics (`P0001`). Use `b.at(0).to_int()` to turn a length byte into an `Int`.
- `wrapping_to_u8()`, `wrapping_to_u16()` and `wrapping_to_u32()`, from a wider unsigned type, keep the low bits.

`U8`, `U32` and `U64` have the bitwise operators `&`, `|`, `^` and `~`, the shifts `<<` and `>>`, and the rotations `rotl(n)` and `rotr(n)`. A shift amount has the shifted value's type; a rotation amount is an `Int`. An amount below 0 or not below the type's width (8, 32 or 64) panics (`P0008`); it is never taken modulo the width. `U16` has the conversions and the endian reads, but no bitwise operators. `Int` and the signed types have none either (`E0223`).

`+`, `-` and `*` are checked, and panic on overflow (`P0001`). Hashes and ciphers wrap on purpose, so they say so: `wrapping_add`, `wrapping_sub` and `wrapping_mul`, on `U8`, `U32` and `U64` (not `U16`).

Where a bitwise or shift operator meets a different binary operator, parentheses are required, so that `x & mask == 0` cannot mean something other than what it looks like:

```oska error=E0108
fn is_even(x: U32) -> Bool {
    x & 1 == 0
}
```

`oskac fix` adds the parentheses Rust's precedence implies: `(x & 1) == 0`. A chain of one operator, such as `a ^ b ^ c`, needs none.

## A worked example

This program builds a record with a 2-byte big-endian length, a 4-byte little-endian tag and a payload, then parses it back and computes a small checksum with wrapping and rotation.

```oska
struct Record { tag: U32, payload: Bytes }

fn encode(record: Record) -> Bytes {
    var out = Bytes.new()
    out.push_u16_be(record.payload.len().to_u16())
    out.push_u32_le(record.tag)
    out.append(record.payload)
    out
}

fn decode(raw: Bytes) -> Option<Record> {
    if raw.len() < 6 {
        return None
    }
    let size = raw.read_u16_be(0).to_int()
    if raw.len() != 6 + size {
        return None
    }
    Some(Record(tag: raw.read_u32_le(2), payload: raw.slice(6, 6 + size)))
}

fn checksum(data: Bytes) -> U32 {
    var sum: U32 = 0
    for i in 0..data.len() {
        sum = sum.rotl(5) ^ data.at(i).to_u32()
        sum = sum.wrapping_mul(16777619)
    }
    sum
}

fn main(io: Stdout) -> Result<(), IoError> {
    let wire = encode(Record(tag: 7, payload: "ping".to_bytes()))
    let hex = wire.to_hex()
    io.println(hex)?
    match decode(wire) {
        Some(record) => {
            let tag = record.tag.to_int()
            let sum = checksum(record.payload).to_int()
            io.println("tag {tag}, checksum {sum}")?
        }
        None => io.println("malformed")?
    }
    Ok(())
}
```

```console cmd=run
00040700000070696e67
tag 7, checksum 2479420452
```

## Bytes from outside

Bytes from a capability carry the trust flag that text does (see [untrusted data](untrusted-data.md)). `DirRead.read_bytes(path)` and `FileRead.read_bytes()` give `Untrusted<Bytes>`, with no newline translation, and `DirWrite.write_bytes(path, contents)` and `FileWrite.write_bytes(contents)` write trusted or untrusted bytes. `Connection.send_bytes` and `receive_bytes` are declared beside `send` and `receive`, but the network has no runtime yet, so a program that asks for `Net` is refused (`E0370`).

- Every `Bytes` operation keeps the flag: `slice`, `append`, `to_hex` and `to_text` of untrusted bytes are untrusted, and `to_text()` gives `Option<Untrusted<String>>`.
- The integers and `Bool`s it gives are trusted: `len`, `at`, `get` and the endian reads. Parsing a binary format needs no trust assertion.
- Untrusted bytes where trusted `Bytes` is required are `E0300`. A validator vouches for bytes it has checked with `.assume_trusted()`, an **audit site** that `oskac check` lists.

```oska
fn magic(raw: Untrusted<Bytes>) -> Bool {
    let png = Bytes.from_hex("89504e47").unwrap_or(Bytes.new())
    raw.len() >= 4 && raw.slice(0, 4) == png
}

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let dir = fs.restrict_to(".")
    let raw = dir.read_bytes(Path.new("image.png"))?
    if magic(raw) {
        io.println("a PNG")?
    }
    Ok(())
}
```

```oska error=E0300
fn header(raw: Untrusted<Bytes>) -> Bytes {
    raw.slice(0, 4)
}
```
