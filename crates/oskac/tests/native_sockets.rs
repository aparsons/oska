//! Native network sockets (ticket #238 of spec #228, ADR 0023): a native
//! program connects and listens exactly as an interpreted one does.
//!
//! Each case runs on both backends through the differential harness
//! (seam 1), which requires byte-identical output, and then asserts what
//! the output says, so a case that failed the same way on both would still
//! fail here. Only the loopback address is used: a program listens on it,
//! connects to the port it was given, and talks to itself from one thread,
//! which a loopback connection allows because `connect` completes against
//! the listener's backlog before `accept`.
//!
//! The same cases run on aarch64 and x86_64 Linux and macOS (system calls
//! and `libSystem`) and on x86_64 Windows (Winsock), which is what the
//! messages having one wording is for. Name resolution is not tested
//! beyond `localhost`: the Oska stub resolver is a later ticket.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::path::{Path, PathBuf};
use std::process::Command;

use differential::{assert_same, Case};

/// A single-file program. A Windows process needs `SYSTEMROOT` to start
/// Winsock, which the harness otherwise clears from the environment.
fn case(name: &str, src: &str) -> Case {
    match std::env::var("SYSTEMROOT") {
        Ok(root) => Case::file(name, src).with_env("SYSTEMROOT", &root),
        Err(_) => Case::file(name, src),
    }
}

/// What a program prints is the same on both backends, and is `expected`.
fn prints(name: &str, src: &str, expected: &str) {
    let outcome = assert_same(&case(name, src));
    assert_eq!(outcome.exit_code, Some(0), "{name}: {outcome:#?}");
    assert_eq!(outcome.stdout, expected, "{name}: {outcome:#?}");
    assert_eq!(outcome.stderr, "", "{name}");
}

#[test]
fn a_connection_carries_text_and_bytes_both_ways() {
    prints(
        "sockets_round_trip",
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let host = net.restrict_to("127.0.0.1", listener.port())
    io.println(host.name())?
    let client = host.connect()?
    let server = listener.accept()?
    io.println(client.host())?
    io.println(server.host())?
    client.send("ping")?
    io.println(server.receive()?)?
    server.send_bytes("pong".to_bytes())?
    io.println(client.receive_bytes()?.assume_trusted().to_hex())?
    client.send_bytes(Bytes.from_hex("00ff7f80").unwrap_or(Bytes.new()))?
    io.println(server.receive_bytes()?.assume_trusted().to_hex())?
    Ok(())
}
"#,
        "127.0.0.1\n127.0.0.1\nloopback\nping\n706f6e67\n00ff7f80\n",
    );
}

#[test]
fn the_end_of_the_stream_is_an_empty_read_and_a_closed_connection_fails() {
    prints(
        "sockets_close",
        r#"fn say(io: Stdout, what: String, r: Result<(), IoError>) -> Result<(), IoError> {
    match r {
        Ok(_) => io.println("{what}: ok")
        Err(e) => {
            let message = e.message
            io.println("{what}: {message}")
        }
    }
}

fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.connect_only().restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    client.send("last words")?
    client.close()
    client.close()
    io.println(server.receive()?)?
    let end = server.receive()?
    io.println("end: [{end}]")?
    let bytes = server.receive_bytes()?
    let count = bytes.assume_trusted().len()
    io.println("end bytes: {count}")?
    say(io, "send after close", client.send("more"))?
    match client.receive() {
        Ok(_) => io.println("receive after close: ok")?
        Err(e) => {
            let message = e.message
            io.println("receive after close: {message}")?
        }
    }
    say(io, "send to a closed peer", server.send("hello?"))?
    Ok(())
}
"#,
        "last words\nend: []\nend bytes: 0\nsend after close: the connection is closed\n\
         receive after close: the connection is closed\nsend to a closed peer: ok\n",
    );
}

#[test]
fn text_that_is_not_utf8_is_an_error_and_bytes_are_not() {
    prints(
        "sockets_not_utf8",
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    server.send_bytes(Bytes.from_hex("fffe").unwrap_or(Bytes.new()))?
    match client.receive() {
        Ok(_) => io.println("text")?
        Err(e) => {
            let message = e.message
            io.println(message)?
        }
    }
    server.send_bytes(Bytes.from_hex("fffe").unwrap_or(Bytes.new()))?
    io.println(client.receive_bytes()?.assume_trusted().to_hex())?
    Ok(())
}
"#,
        "stream did not contain valid UTF-8\nfffe\n",
    );
}

#[test]
fn a_long_message_arrives_whole_across_several_reads() {
    prints(
        "sockets_long",
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    let message = "0123456789abcdefghij".repeat(2000)
    client.send(message)?
    var received = Bytes.new()
    while received.len() < 40000 {
        let chunk = server.receive_bytes()?.assume_trusted()
        if chunk.len() == 0 {
            break
        }
        received.append(chunk)
    }
    let count = received.len()
    var same = "different"
    if received.to_text().unwrap_or("") == message {
        same = "same"
    }
    io.println("{count} {same}")?
    Ok(())
}
"#,
        "40000 same\n",
    );
}

#[test]
fn localhost_reaches_a_listener_on_the_loopback_address() {
    prints(
        "sockets_localhost",
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let host = net.restrict_to("localhost", listener.port())
    let client = host.connect()?
    let server = listener.accept()?
    io.println(client.host())?
    client.send("hi")?
    io.println(server.receive()?)?
    Ok(())
}
"#,
        "localhost\nhi\n",
    );
}

/// What a program that makes each kind of bad connection prints: the
/// wording is Oska's, from its table, so it is the same on every platform
/// and on both backends. The port a connection is refused at is whichever
/// the system gave, so the program names it `PORT`.
#[test]
fn failures_read_the_same_on_every_platform() {
    const INVALID: &str = "it is not a DNS name or an IP address";
    let long = "a".repeat(64);
    let expected = format!(
        "refused: cannot connect to 127.0.0.1:PORT: connection refused\n\
         port 0: port 0 is out of range: use 1 to 65535\n\
         port 65536: port 65536 is out of range: use 1 to 65535\n\
         port -3: port -3 is out of range: use 1 to 65535\n\
         empty host: invalid host \"\": {INVALID}\n\
         space: invalid host \"a b\": {INVALID}\n\
         url: invalid host \"http://x\": {INVALID}\n\
         colons: invalid host \":::\": {INVALID}\n\
         too many groups: invalid host \"1:2:3:4:5:6:7:8:9\": {INVALID}\n\
         two gaps: invalid host \"1::2::3\": {INVALID}\n\
         long label: invalid host \"{long}.com\": {INVALID}\n\
         not ascii: invalid host \"héllo.com\": {INVALID}\n\
         quote: invalid host \"a\\\"b\": {INVALID}\n\
         listen on 70000: port 70000 is out of range: use 0 to 65535\n\
         no port when it could not listen: 0\n"
    );
    prints(
        "sockets_failures",
        r#"fn show(io: Stdout, what: String, port: Int, r: Result<Connection, IoError>) -> Result<(), IoError> {
    match r {
        Ok(_) => io.println("{what}: connected")
        Err(e) => {
            let message = e.message.replace("{port}", "PORT")
            io.println("{what}: {message}")
        }
    }
}

fn free_port(net: Net) -> Int {
    let listener = net.listen_only().restrict_to_loopback(0)
    listener.port()
}

fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let p = free_port(net)
    show(io, "refused", p, net.restrict_to("127.0.0.1", p).connect())?
    show(io, "port 0", p, net.restrict_to("127.0.0.1", 0).connect())?
    show(io, "port 65536", p, net.restrict_to("127.0.0.1", 65536).connect())?
    show(io, "port -3", p, net.restrict_to("127.0.0.1", -3).connect())?
    show(io, "empty host", p, net.restrict_to("", 80).connect())?
    show(io, "space", p, net.restrict_to("a b", 80).connect())?
    show(io, "url", p, net.restrict_to("http://x", 80).connect())?
    show(io, "colons", p, net.restrict_to(":::", 80).connect())?
    show(io, "too many groups", p, net.restrict_to("1:2:3:4:5:6:7:8:9", 80).connect())?
    show(io, "two gaps", p, net.restrict_to("1::2::3", 80).connect())?
    let long = "a".repeat(64)
    show(io, "long label", p, net.restrict_to("{long}.com", 80).connect())?
    show(io, "not ascii", p, net.restrict_to("héllo.com", 80).connect())?
    show(io, "quote", p, net.restrict_to("a\"b", 80).connect())?
    let big = net.listen_only().restrict_to_loopback(70000)
    match big.accept() {
        Ok(_) => io.println("listen on 70000: accepted")?
        Err(e) => {
            let message = e.message
            io.println("listen on 70000: {message}")?
        }
    }
    let first = big.port()
    io.println("no port when it could not listen: {first}")?
    Ok(())
}
"#,
        &expected,
    );
}

/// An IPv6 literal, in any of its forms, is an address and not a name; an
/// IPv4 address is four numbers without leading zeros. Whether the machine
/// can reach `::1` is its own business, so this requires only that both
/// backends agree and that none of the valid forms is refused as a host.
#[test]
fn ip_literals_are_parsed_as_rust_parses_them() {
    let outcome = assert_same(&case(
        "sockets_ip_literals",
        r#"fn free_port(net: Net) -> Int {
    let listener = net.listen_only().restrict_to_loopback(0)
    listener.port()
}

fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let p = free_port(net)
    let valid = ["::1", "0:0:0:0:0:0:0:1", "0::1", "::ffff:127.0.0.1", "0:0:0:0:0:ffff:7f00:1", "::0.0.0.1", "::", "1::", "127.0.0.1", "0.0.0.0"]
    for host in valid {
        match net.restrict_to(host, p).connect() {
            Ok(_) => io.println("{host}: connected")?
            Err(e) => {
                let message = e.message.replace("{p}", "PORT")
                io.println("{host}: {message}")?
            }
        }
    }
    let invalid = ["1:2:3:4:5:6:7", "1:2:3:4:5:6:7:8:9", ":1", "1:", "1:::2", "::1::", "12345::1", "::g", "::1.2.3", "::1.2.3.4.5", "::256.1.1.1", ":::", "::1%lo", "[::1]", " ::1"]
    for host in invalid {
        match net.restrict_to(host, p).connect() {
            Ok(_) => io.println("{host}: connected")?
            Err(e) => {
                let message = e.message.replace("{p}", "PORT")
                io.println("{host}: {message}")?
            }
        }
    }
    Ok(())
}
"#,
    ));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    for line in outcome.stdout.lines() {
        let (host, message) = line.split_once(": ").expect("a host and a message");
        let rejected = message.starts_with("invalid host");
        let valid = [
            "::1",
            "0:0:0:0:0:0:0:1",
            "0::1",
            "::ffff:127.0.0.1",
            "0:0:0:0:0:ffff:7f00:1",
            "::0.0.0.1",
            "::",
            "1::",
            "127.0.0.1",
            "0.0.0.0",
        ];
        // Eight of the invalid forms hold a colon and so are no DNS name:
        // they are refused as hosts. The others hold none and are well
        // formed names, which the runtime does not resolve (yet).
        assert_eq!(
            valid.contains(&host),
            !rejected || !host.contains(':'),
            "{line}"
        );
        if valid.contains(&host) {
            assert!(!rejected && !message.contains("host not found"), "{line}");
        }
    }
}

/// What `oskac test` shows for `src` with `flags`, on the interpreter or
/// natively: its output with the durations masked, and its exit code.
fn oskac_test(name: &str, src: &str, flags: &[&str], interpret: bool) -> (String, Option<i32>) {
    let dir: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("native_sockets")
        .join(format!(
            "{name}-{}",
            if interpret { "interpreted" } else { "native" }
        ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    std::fs::write(dir.join("main.osk"), src).expect("should write the program");
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command
        .current_dir(&dir)
        .arg("test")
        .arg("main.osk")
        .args(flags);
    if interpret {
        command.arg("--interpret");
    }
    command.env_clear();
    if let Ok(root) = std::env::var("SYSTEMROOT") {
        command.env("SYSTEMROOT", root);
    }
    let output = command.output().expect("oskac should run");
    let mut shown = common::golden::mask_durations(&String::from_utf8_lossy(&output.stdout));
    shown.push_str(&String::from_utf8_lossy(&output.stderr));
    (shown, output.status.code())
}

/// A plain test is offline: `t.net` refuses every connection and listen
/// with one stable error, on both backends; under `--integration` it is
/// the real network, which a test reaches on the loopback address.
#[test]
fn a_plain_test_is_offline_and_an_integration_test_reaches_the_loopback() {
    let src = r#"
fn why(r: Result<Connection, IoError>) -> String {
    match r {
        Ok(_) => "connected"
        Err(e) => e.message.assume_trusted()
    }
}

test "a connection is refused" {
    let reached = t.net.restrict_to("example.com", 443).connect()
    t.assert_eq(why(reached), "network access is not available here: a plain test is offline")
}

test "a listener is refused" {
    let listener = t.net.listen_only().restrict_to_loopback(0)
    match listener.accept() {
        Ok(_) => t.fail("accepted")
        Err(e) => t.assert_eq(e.message.assume_trusted(), "network access is not available here: a plain test is offline")
    }
    t.assert_eq(listener.port(), 0)
}

#[integration]
test "the real network reaches the loopback" {
    let listener = t.net.listen_only().restrict_to_loopback(0)
    match t.net.restrict_to("127.0.0.1", listener.port()).connect() {
        Ok(client) => {
            match client.send("ping") {
                Ok(_) => t.assert(true)
                Err(e) => t.fail(e.message.assume_trusted())
            }
        }
        Err(e) => t.fail(e.message.assume_trusted())
    }
}
"#;
    for flags in [&[][..], &["--integration"][..]] {
        let interpreted = oskac_test("sockets_tests", src, flags, true);
        let native = oskac_test("sockets_tests", src, flags, false);
        assert_eq!(interpreted, native, "{flags:?}");
        assert_eq!(interpreted.1, Some(0), "{flags:?}: {interpreted:#?}");
        assert!(
            interpreted
                .0
                .contains("test a connection is refused ... ok"),
            "{flags:?}: {interpreted:#?}"
        );
    }
}

/// Listening twice on one port is refused where the machine has no second
/// loopback address to fall back on, and is not where `::1` is there, so
/// what it gives is the machine's own business: both backends must agree.
#[test]
fn listening_on_a_taken_port_gives_the_same_result_on_both_backends() {
    let outcome = assert_same(&case(
        "sockets_taken_port",
        r#"fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let taken = net.listen_only().restrict_to_loopback(0)
    let again = net.listen_only().restrict_to_loopback(taken.port())
    let port = again.port()
    if port == 0 {
        io.println("refused")
    } else {
        io.println("listening")
    }
}
"#,
    ));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    // Where the machine has no `::1`, the second bind has nowhere to fall
    // back to and is refused on Linux and macOS, where the port is held
    // without `SO_REUSEADDR` sharing. Windows' rules differ.
    let has_ipv6_loopback = std::net::TcpListener::bind("[::1]:0").is_ok();
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        let expected = if has_ipv6_loopback {
            "listening\n"
        } else {
            "refused\n"
        };
        assert_eq!(outcome.stdout, expected, "{outcome:#?}");
    }
}
