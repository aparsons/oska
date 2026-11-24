// SPDX-License-Identifier: Apache-2.0
//! `std.dns` (#239) as programs run: the message, address and file
//! parsers are pure Oska, linked into the program and run by both
//! backends with the same output. The recorded messages and hostile
//! answers are test-bearing fixtures (`fixtures/dns_messages.osk`), which
//! the native test corpus runs on both backends; this file runs whole
//! programs through the differential harness, with an answer from the
//! command line and files from stdin, and builds one for every target.
//! `HostConnect.exchange_datagram` runs natively too (#690): the tests
//! near the end send a datagram to a local UDP server and read the reply
//! on both backends. The resolver over real sockets is the interpreter's
//! (`crates/oska-run/tests/dns_loopback.rs`).

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::net::{Ipv4Addr, UdpSocket};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::portable::Scratch;
use differential::{assert_same, Case};

/// An answer from the command line, for `example.com` or the name in
/// argument 2 (type A, or AAAA with argument 3 `28`): its addresses, or
/// the error's wording.
const PARSE: &str = r#"use std.dns

fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    let reply = match args.get(1) {
        Some(text) => Bytes.from_hex(text.assume_trusted()).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let name = match args.get(2) {
        Some(text) => text.assume_trusted()
        None => "example.com"
    }
    let kind = match args.get(3) {
        Some(_) => dns.TYPE_AAAA
        None => dns.TYPE_A
    }
    match dns.parse_response(reply, 4660, name, kind) {
        Ok(answer) => {
            let addresses = " ".join(answer.addresses)
            let ttl = answer.ttl
            io.println("{addresses} ttl {ttl}")
        }
        Err(error) => io.println("error: {error}")
    }
}
"#;

fn parse(name: &str, reply: &str, host: &str, kind: &[&str]) -> Case {
    let mut args = vec![reply, host];
    args.extend_from_slice(kind);
    Case::file(name, PARSE).with_args(&args)
}

#[test]
fn an_answer_is_read_the_same_on_both_backends() {
    for (name, reply, host, kind, expected) in [
        ("dns_a", "123481800001000200000000076578616d706c6503636f6d0000010001c00c0001000100000e1000045db8d822c00c000100010000025800045db8d823", "example.com", &[][..], "93.184.216.34 93.184.216.35 ttl 600"),
        ("dns_aaaa", "123481800001000100000000076578616d706c6503636f6d00001c0001c00c001c00010000012c001026062800022000010248189325c81946", "example.com", &["28"][..], "2606:2800:220:1:248:1893:25c8:1946 ttl 300"),
        ("dns_cname", "12348180000100030000000003777777076578616d706c6503636f6d0000010001c00c0005000100000078001103776562076578616d706c65036e65740003776562076578616d706c65036e657400000500010000003c00110363646e076578616d706c65036f7267000363646e076578616d706c65036f726700000100010000001e0004c6336407", "www.example.com", &[][..], "198.51.100.7 ttl 30"),
        ("dns_nx", "123481830001000000000000046e6f7065076578616d706c6503636f6d0000010001", "nope.example.com", &[][..], "error: host not found"),
        ("dns_loop", "123481800001000100000000076578616d706c6503636f6d00000100010161c01f000100010000012c000401020304", "example.com", &[][..], "error: malformed answer"),
        ("dns_count", "123481800001ffff00000000076578616d706c6503636f6d0000010001c00c000100010000012c000401020304", "example.com", &[][..], "error: malformed answer"),
        ("dns_cut", "123481800001000200000000076578616d706c6503636f6d0000010001c00c0001000100000e1000045db8d822c00c000100010000025800045d", "example.com", &[][..], "error: malformed answer"),
        ("dns_empty", "", "example.com", &[][..], "error: malformed answer"),
    ] {
        let outcome = assert_same(&parse(name, reply, host, kind));
        assert_eq!(outcome.stdout, format!("{expected}\n"), "{name}: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(0), "{name}: {outcome:#?}");
    }
}

/// A hosts file and a `resolv.conf` from stdin, split by a line of `---`:
/// what the lookups and the configuration say.
const CONFIG: &str = r#"use std.dns

fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    var hosts = ""
    var resolv = ""
    var second = false
    var more = true
    while more {
        match input.read_line()? {
            Some(line) => {
                if line.trim() == "---" {
                    second = true
                } else if second {
                    resolv = "{resolv}{line}\n"
                } else {
                    hosts = "{hosts}{line}\n"
                }
            }
            None => {
                more = false
            }
        }
    }
    let config = dns.Config.new(hosts, resolv, ["10.0.0.9"])
    let found = " ".join(dns.lookup_hosts(config.hosts, "web.example.com"))
    let servers = " ".join(config.servers)
    let search = " ".join(dns.candidates(config, "www", false))
    let ndots = config.ndots
    io.println("hosts: {found}")?
    io.println("servers: {servers}")?
    io.println("asked: {search}")?
    io.println("ndots: {ndots}")
}
"#;

#[test]
fn a_hosts_file_and_a_resolv_conf_are_read_the_same_on_both_backends() {
    let stdin = "127.0.0.1 localhost\n10.0.0.5 Web.Example.COM. web # a comment\n\
                 2001:db8::5 web.example.com\nfe80::1%lo0 zoned\n---\n\
                 # resolv.conf\nnameserver 192.0.2.1\nnameserver 2001:db8::53\n\
                 search corp.test lab.test\noptions ndots:2\n";
    let outcome = assert_same(&Case::file("dns_config", CONFIG).with_stdin(stdin));
    assert_eq!(
        outcome.stdout,
        "hosts: 2001:db8::5 10.0.0.5\nservers: 192.0.2.1 2001:db8::53\n\
         asked: www.corp.test www.lab.test www\nndots: 2\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_configuration_with_no_name_server_uses_the_systems_then_the_loopback() {
    let src = r#"use std.dns

fn main(io: Stdout) -> Result<(), IoError> {
    let system = dns.Config.new("", "", ["10.0.0.9", "not an address"])
    let none = dns.Config.new("", "", [])
    let a = " ".join(system.servers)
    let b = " ".join(none.servers)
    io.println("{a} | {b}")
}
"#;
    let outcome = assert_same(&Case::file("dns_servers", src));
    assert_eq!(outcome.stdout, "10.0.0.9 | 127.0.0.1\n", "{outcome:#?}");
}

/// `NetConnect.system_name_servers` runs on both backends, which connect
/// nowhere: Windows' DNS servers come from `GetNetworkParams` (ADR 0026),
/// and every other platform keeps them in `resolv.conf`, so it lists none.
/// A test host lists none either, whatever the platform.
#[test]
fn the_systems_name_servers_are_listed_on_windows_only() {
    let src = r#"fn main(io: Stdout, net: NetConnect) -> Result<(), IoError> {
    let servers = net.system_name_servers()?
    let count = servers.len()
    io.println("{count}")
}
"#;
    let outcome = differential::run_native(&Case::file("dns_system_servers", src));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let listed: usize = outcome.stdout.trim().parse().expect("a count");
    assert!(listed <= 16, "{outcome:#?}");
    if !cfg!(windows) {
        assert_eq!(outcome.stdout, "0\n", "{outcome:#?}");
    }
}

/// A program using `std.dns` builds for every target with `--target all`,
/// with no internal error, and two builds write the same bytes.
#[test]
fn std_dns_builds_for_every_target_the_same_twice() {
    let dir = Scratch::new("oska-dns", "all");
    std::fs::write(dir.join("lookup.osk"), PARSE).expect("write the program");
    let mut builds = Vec::new();
    for out in ["first", "second"] {
        std::fs::create_dir(dir.join(out)).expect("make the output directory");
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "lookup.osk", "--target", "all", "-o", out])
            .env("OSKA_CACHE_DIR", dir.join(format!("cache-{out}")))
            .current_dir(&dir)
            .output()
            .expect("oskac runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{output:?}");
        assert!(!stderr.contains("E0398"), "{stderr}");
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir.join(out))
            .expect("the output directory")
            .map(|e| {
                let path = e.expect("an entry").path();
                let name = path
                    .file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned();
                (name, std::fs::read(&path).expect("an executable"))
            })
            .collect();
        files.sort();
        builds.push(files);
    }
    assert_eq!(builds[0].len(), 5, "one executable per target");
    assert!(builds[0] == builds[1], "two builds differ");
}

/// A local UDP server on the loopback address that answers every datagram
/// with a forged one (another transaction id) and then the message with
/// `!` appended, until it is dropped. A silent one never answers.
struct UdpServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl UdpServer {
    fn start(silent: bool) -> UdpServer {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a UDP port");
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("a read timeout");
        let port = socket.local_addr().expect("an address").port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 2048];
            while !flag.load(Ordering::Relaxed) {
                let Ok((n, from)) = socket.recv_from(&mut buffer) else {
                    continue;
                };
                if silent {
                    continue;
                }
                let mut forged = buffer[..n].to_vec();
                forged[0] ^= 0xff;
                let mut answer = buffer[..n].to_vec();
                answer.push(b'!');
                let _ = socket.send_to(&forged, from);
                let _ = socket.send_to(&answer, from);
            }
        });
        UdpServer {
            port,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for UdpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A program that sends the hex in argument 2 to `host` at the port in
/// argument 1 and prints the answer's hex, or the error's wording. A
/// Windows process needs `SYSTEMROOT` to start Winsock.
fn exchange(name: &str, host: &str, port: u16, hex: &str) -> Case {
    let src = format!(
        r#"fn main(io: Stdout, net: Net, args: Args) -> Result<(), IoError> {{
    let port = match args.get(1) {{
        Some(text) => text.assume_trusted().parse_int().unwrap_or(0)
        None => 0
    }}
    let message = match args.get(2) {{
        Some(text) => Bytes.from_hex(text.assume_trusted()).unwrap_or(Bytes.new())
        None => Bytes.new()
    }}
    match net.restrict_to("{host}", port).exchange_datagram(message) {{
        Ok(answer) => io.println(answer.assume_trusted().to_hex())
        Err(e) => io.println("error: {{e.message}}")
    }}
}}
"#
    );
    let case = Case::file(name, &src).with_args(&[&port.to_string(), hex]);
    match std::env::var("SYSTEMROOT") {
        Ok(root) => case.with_env("SYSTEMROOT", &root),
        Err(_) => case,
    }
}

/// The first datagram whose first two bytes are the message's own comes
/// back, the forged one before it skipped, the same on both backends.
#[test]
fn a_datagram_gets_the_matching_answer_on_both_backends() {
    let server = UdpServer::start(false);
    let outcome = assert_same(&exchange(
        "dns_datagram",
        "127.0.0.1",
        server.port,
        "1234010001",
    ));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stdout, "123401000121\n", "{outcome:#?}");
}

#[test]
fn a_datagram_nobody_answers_times_out_on_both_backends() {
    let server = UdpServer::start(true);
    let outcome = assert_same(&exchange("dns_silent", "127.0.0.1", server.port, "1234"));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let expected = format!("error: cannot reach 127.0.0.1:{}: timed out\n", server.port);
    assert_eq!(outcome.stdout, expected, "{outcome:#?}");
}

#[test]
fn a_datagram_refuses_a_name_a_bad_port_and_a_long_message() {
    for (name, host, port, hex, expected) in [
        (
            "dns_name",
            "example.com",
            53,
            "1234",
            "error: invalid host \"example.com\": a datagram goes to an IP address\n",
        ),
        (
            "dns_bad_host",
            "not a host",
            53,
            "1234",
            "error: invalid host \"not a host\": it is not a DNS name or an IP address\n",
        ),
        (
            "dns_port",
            "127.0.0.1",
            0,
            "1234",
            "error: port 0 is out of range: use 1 to 65535\n",
        ),
    ] {
        let outcome = assert_same(&exchange(name, host, port, hex));
        assert_eq!(outcome.stdout, expected, "{name}: {outcome:#?}");
    }
    let long = "ab".repeat(65508);
    let outcome = assert_same(&exchange("dns_long", "127.0.0.1", 53, &long));
    assert_eq!(
        outcome.stdout, "error: the datagram is too long to send\n",
        "{:?}",
        outcome.exit_code
    );
}
