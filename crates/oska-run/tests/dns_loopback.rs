//! `std.dns` against a name server on the loopback address (#239): a
//! Rust thread answers over UDP and TCP, and an Oska program resolves
//! names through `NetExchange` on real sockets, with the hosts file and
//! `resolv.conf` given as text. The interpreter only: the native backend
//! has no networking yet (`E0390`), which the parity gate reports. Real
//! lookups of public names run only under `--integration`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use oska_run::{test, RealHost, TestOptions, TestStatus};

/// A name server on `127.0.0.1`, one port for UDP and TCP, answering from
/// a small zone until dropped.
struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

/// What the server knows: `example.com` has `93.184.216.34` and
/// `2001:db8::1`; `big.example.com` has two IPv4 addresses and its UDP
/// answer is truncated; `nope.example.com` does not exist;
/// `forged.example.com` is answered first with another id; the others
/// are refused.
fn answer(query: &[u8], udp: bool) -> Vec<Vec<u8>> {
    let id = [query[0], query[1]];
    let mut at = 12;
    let mut labels = Vec::new();
    while query[at] != 0 {
        let len = query[at] as usize;
        labels.push(String::from_utf8_lossy(&query[at + 1..at + 1 + len]).to_string());
        at += 1 + len;
    }
    let question = &query[12..at + 5];
    let kind = u16::from_be_bytes([query[at + 1], query[at + 2]]);
    let name = labels.join(".");
    let reply = |rcode: u8, truncated: bool, records: &[Vec<u8>], id: [u8; 2]| {
        let mut out = vec![id[0], id[1], 0x81 | u8::from(truncated) << 1, 0x80 | rcode];
        out.extend_from_slice(&[0, 1]);
        out.extend_from_slice(&(records.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(question);
        for record in records {
            out.extend_from_slice(record);
        }
        out
    };
    let record = |kind: u16, data: &[u8]| {
        let mut out = vec![0xc0, 0x0c];
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 1, 0x2c]);
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
        out
    };
    let v6: [u8; 16] = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    match (name.as_str(), kind) {
        ("example.com", 1) => vec![reply(0, false, &[record(1, &[93, 184, 216, 34])], id)],
        ("example.com", 28) => vec![reply(0, false, &[record(28, &v6)], id)],
        ("big.example.com", 1) if udp => vec![reply(0, true, &[], id)],
        ("big.example.com", 1) => vec![reply(
            0,
            false,
            &[record(1, &[192, 0, 2, 1]), record(1, &[192, 0, 2, 2])],
            id,
        )],
        ("big.example.com", _) => vec![reply(0, false, &[], id)],
        ("nope.example.com", _) => vec![reply(3, false, &[], id)],
        ("forged.example.com", 1) => vec![
            reply(
                0,
                false,
                &[record(1, &[203, 0, 113, 66])],
                [id[0] ^ 1, id[1]],
            ),
            reply(0, false, &[record(1, &[192, 0, 2, 44])], id),
        ],
        ("forged.example.com", _) => vec![reply(0, false, &[], id)],
        _ => vec![reply(5, false, &[], id)],
    }
}

impl Server {
    fn start() -> Server {
        let (udp, tcp) = loop {
            let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("udp");
            let port = udp.local_addr().expect("address").port();
            if let Ok(tcp) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                break (udp, tcp);
            }
        };
        let port = udp.local_addr().expect("address").port();
        udp.set_read_timeout(Some(Duration::from_millis(50)))
            .expect("timeout");
        tcp.set_nonblocking(true).expect("nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let udp_stop = stop.clone();
        let udp_thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            while !udp_stop.load(Ordering::Relaxed) {
                if let Ok((read, peer)) = udp.recv_from(&mut buffer) {
                    for reply in answer(&buffer[..read], true) {
                        let _ = udp.send_to(&reply, peer);
                    }
                }
            }
        });
        let tcp_stop = stop.clone();
        let tcp_thread = std::thread::spawn(move || {
            while !tcp_stop.load(Ordering::Relaxed) {
                match tcp.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).expect("blocking");
                        socket
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .expect("timeout");
                        serve_stream(&mut socket);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Server {
            port,
            stop,
            threads: vec![udp_thread, tcp_thread],
        }
    }
}

/// One TCP query: a two-byte length, then the message, and the answer the
/// same way. The answer is sent in two writes, so the client must read
/// until the whole message has arrived.
fn serve_stream(socket: &mut TcpStream) {
    let mut length = [0u8; 2];
    if socket.read_exact(&mut length).is_err() {
        return;
    }
    let mut query = vec![0u8; u16::from_be_bytes(length) as usize];
    if socket.read_exact(&mut query).is_err() {
        return;
    }
    let reply = &answer(&query, false)[0];
    let mut framed = (reply.len() as u16).to_be_bytes().to_vec();
    framed.extend_from_slice(&reply[..5]);
    let _ = socket.write_all(&framed);
    let _ = socket.flush();
    std::thread::sleep(Duration::from_millis(20));
    let _ = socket.write_all(&reply[5..]);
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Resolve each name in `cases` through the server at `port` in an
/// `#[integration]` test, which gets the real network (loopback only
/// here), and check what comes back: the addresses, or `!` and the error.
fn resolves(port: u16, resolv_conf: &str, hosts: &str, cases: &[(&str, &str)]) {
    let mut body = String::new();
    for (name, expected) in cases {
        body.push_str(&format!(
            "    t.assert_eq(lookup(config, dns.NetExchange.on_port(t.net.connect_only(), t.random, {port}), \"{name}\"), \"{expected}\")\n"
        ));
    }
    let src = format!(
        "use std.dns\n\n\
         fn lookup(config: dns.Config, transport: dns.NetExchange, name: String) -> String {{\n\
         \x20   match dns.resolve(config, transport, name) {{\n\
         \x20       Ok(found) => \" \".join(found)\n\
         \x20       Err(error) => \"!{{error}}\"\n\
         \x20   }}\n\
         }}\n\n\
         #[integration]\n\
         test \"resolves\" {{\n\
         \x20   let config = dns.Config.new(\"{hosts}\", \"{resolv_conf}\", [])\n{body}}}\n"
    );
    let report = test(
        &src,
        &mut RealHost::new(),
        TestOptions {
            integration: true,
            ..TestOptions::default()
        },
    );
    assert!(
        report.diagnostics.is_empty()
            && report.tests.len() == 1
            && report.tests[0].status == TestStatus::Passed,
        "{report:#?}"
    );
}

#[test]
fn names_resolve_over_udp_from_a_loopback_name_server() {
    let server = Server::start();
    resolves(
        server.port,
        "nameserver 127.0.0.1\\n",
        "",
        &[
            ("example.com", "2001:db8::1 93.184.216.34"),
            ("nope.example.com", "!host not found"),
            ("other.test", "!name server refused"),
        ],
    );
}

#[test]
fn a_truncated_answer_is_asked_again_over_tcp_and_read_in_pieces() {
    let server = Server::start();
    resolves(
        server.port,
        "nameserver 127.0.0.1\\n",
        "",
        &[("big.example.com", "192.0.2.1 192.0.2.2")],
    );
}

#[test]
fn an_answer_with_another_id_is_ignored_for_the_real_one() {
    let server = Server::start();
    // The forged answer arrives first from the right address and port, so
    // the id check is what refuses it, and the attempt after it succeeds.
    resolves(
        server.port,
        "nameserver 127.0.0.1\\n",
        "",
        &[("forged.example.com", "192.0.2.44")],
    );
}

#[test]
fn the_hosts_file_and_address_literals_ask_no_name_server() {
    // Nothing listens on this port: a name server is never asked.
    let free = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
    let port = free.local_addr().expect("address").port();
    resolves(
        port,
        "nameserver 127.0.0.1\\n",
        "10.1.1.1 printer.lan\\n",
        &[("printer.lan", "10.1.1.1"), ("192.0.2.9", "192.0.2.9")],
    );
}

#[test]
fn a_name_server_that_does_not_answer_times_out_with_oska_wording() {
    // A UDP socket that never answers.
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
    let port = silent.local_addr().expect("address").port();
    resolves(
        port,
        "nameserver 127.0.0.1\\noptions attempts:1\\n",
        "",
        &[("example.com", "!timed out")],
    );
}

/// A real lookup of a public name, through the system's name servers and
/// `resolv.conf`: only under `--integration`.
#[test]
fn a_real_lookup_runs_only_under_integration() {
    let src = "use std.dns\n\n\
        #[integration]\n\
        test \"a public name resolves\" {\n\
        \x20   let config = dns.load_config(t.fs.read_only(), t.net.connect_only())\n\
        \x20   match dns.resolve(config, dns.NetExchange.new(t.net.connect_only(), t.random), \"example.com\") {\n\
        \x20       Ok(found) => t.assert(found.len() > 0)\n\
        \x20       Err(error) => t.fail(\"{error}\")\n\
        \x20   }\n\
        }\n";
    let plain = test(src, &mut RealHost::new(), TestOptions::default());
    assert!(
        plain.tests.iter().all(|t| t.status == TestStatus::Skipped),
        "{plain:#?}"
    );
}
