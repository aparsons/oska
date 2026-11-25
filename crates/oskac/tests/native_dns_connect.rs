// SPDX-License-Identifier: Apache-2.0
//! `connect()` resolves a name with `std.dns` natively too (#691): these
//! tests connect by name to a loopback listener, with a hosts file and a
//! name server of the test's own (`OSKA_DNS_HOSTS`, ...), and compare the
//! backends.

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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use differential::{assert_same, Case};

/// A local UDP name server that answers every query with "no such host"
/// (NXDOMAIN), until it is dropped.
struct NoSuchHost {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl NoSuchHost {
    fn start() -> NoSuchHost {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a UDP port");
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("a read timeout");
        let port = socket.local_addr().expect("an address").port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            while !flag.load(Ordering::Relaxed) {
                let Ok((n, from)) = socket.recv_from(&mut buffer) else {
                    continue;
                };
                if n < 12 {
                    continue;
                }
                // The query's id and question, flags "response, no such
                // name", one question and no answer.
                let mut answer = buffer[..n].to_vec();
                answer[2] = 0x81;
                answer[3] = 0x83;
                answer[6..12].fill(0);
                let _ = socket.send_to(&answer, from);
            }
        });
        NoSuchHost {
            port,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for NoSuchHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A program that listens on the loopback, connects to the name in
/// argument 1 at the listener's port, and prints what the connection did:
/// the host it names and a message that crosses it, or the error with the
/// port shown as `PORT`.
fn connect_by_name(name: &str, host: &str, hosts: &str, port: u16) -> Case {
    let src = r#"fn main(io: Stdout, net: Net, args: Args) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let name = match args.get(1) {
        Some(text) => text.assume_trusted()
        None => "localhost"
    }
    let port = listener.port()
    match net.restrict_to(name, port).connect() {
        Ok(client) => {
            let server = listener.accept()?
            client.send("hi")?
            io.println(client.host())?
            io.println(server.receive()?)
        }
        Err(e) => {
            let message = e.message.replace("{port}", "PORT")
            io.println("error: {message}")
        }
    }
}
"#;
    let case = Case::file(name, src)
        .with_args(&[host])
        .with_env("OSKA_DNS_HOSTS", hosts)
        .with_env("OSKA_DNS_RESOLV_CONF", "nameserver 127.0.0.1\n")
        .with_env("OSKA_DNS_PORT", &port.to_string());
    match std::env::var("SYSTEMROOT") {
        Ok(root) => case.with_env("SYSTEMROOT", &root),
        Err(_) => case,
    }
}

/// A name from the hosts file, and `localhost` from it, reach a loopback
/// listener, the same on both backends.
#[test]
fn a_name_from_the_hosts_file_connects_on_both_backends() {
    let hosts = "127.0.0.1 localhost\n127.0.0.1 printer.invalid\n";
    for (name, host) in [
        ("dns_connect_hosts", "printer.invalid"),
        ("dns_connect_localhost", "localhost"),
    ] {
        let outcome = assert_same(&connect_by_name(name, host, hosts, 9));
        assert_eq!(outcome.exit_code, Some(0), "{name}: {outcome:#?}");
        assert_eq!(
            outcome.stdout,
            format!("{host}\nhi\n"),
            "{name}: {outcome:#?}"
        );
    }
}

/// A name nobody knows is `host not found`, with the interpreter's words.
#[test]
fn an_unknown_name_is_host_not_found_on_both_backends() {
    let server = NoSuchHost::start();
    let outcome = assert_same(&connect_by_name(
        "dns_connect_absent",
        "absent.invalid",
        "127.0.0.1 localhost\n",
        server.port,
    ));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(
        outcome.stdout, "error: cannot connect to absent.invalid:PORT: host not found\n",
        "{outcome:#?}"
    );
}

/// A port that is not one makes every lookup `host not found`, not a
/// silent fall back to port 53.
#[test]
fn a_bad_name_server_port_is_host_not_found_on_both_backends() {
    for (name, port) in [("dns_connect_port0", "0"), ("dns_connect_portx", "x")] {
        let case = connect_by_name(name, "absent.invalid", "127.0.0.1 localhost\n", 9)
            .with_env("OSKA_DNS_PORT", port);
        let outcome = assert_same(&case);
        assert_eq!(
            outcome.stdout, "error: cannot connect to absent.invalid:PORT: host not found\n",
            "{name}: {outcome:#?}"
        );
    }
}
