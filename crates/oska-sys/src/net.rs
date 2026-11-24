// SPDX-License-Identifier: Apache-2.0
//! TCP connections for the Oska runtime, with one wording of every
//! failure on every platform (ADR 0014).
//!
//! The standard library already wraps the sockets of the three operating
//! systems Oska targets, so none of this needs `unsafe`. It lives here so
//! that the runtime reaches the network through one small interface, as it
//! reaches files through [`crate::Dir`]: [`NetStream`] connects to one host
//! and port, and [`NetListener`] accepts on the loopback address only.
//!
//! A failure is worded by [`describe`], from its kind and never from the
//! system's own text or error number, so a program's output does not depend
//! on the operating system. Name resolution is the system's for now:
//! `std.dns`, the Oska stub resolver (ADR 0026), takes it over when the
//! native sockets and the TLS client need it. [`exchange_datagram`] is the
//! UDP half that `std.dns` asks name servers with.

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
    UdpSocket,
};
use std::time::{Duration, Instant};

/// How long `connect` waits for each address to answer.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `connect` takes in all, over every address it tries.
pub const CONNECT_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a read, or an `accept`, waits for the other side.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How long [`exchange_datagram`] waits for the answer.
pub const DATAGRAM_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest datagram [`exchange_datagram`] sends or accepts: what a UDP
/// datagram over IPv4 can carry.
const MAX_DATAGRAM: usize = 65507;

/// The longest a host name may be, as in DNS.
const MAX_HOST_LEN: usize = 253;

/// The longest a DNS label may be.
const MAX_LABEL_LEN: usize = 63;

/// An open connection to one host. Dropping the last copy closes it.
#[derive(Debug)]
pub struct NetStream {
    socket: TcpStream,
    closed: Cell<bool>,
}

impl NetStream {
    /// Connect to `host` at `port`, trying each address the name resolves
    /// to in turn, each for at most [`CONNECT_TIMEOUT`]. `host` is a DNS
    /// name, an IPv4 literal or an IPv6 literal, and `port` is 1 to 65535;
    /// anything else is an error that says so, and no socket is opened.
    pub fn connect(host: &str, port: i128) -> io::Result<NetStream> {
        let port = check_port(port, 1)?;
        let address = parse_host(host)?;
        let failed = |why: &str| {
            io::Error::other(format!(
                "cannot connect to {}: {why}",
                shown_address(host, port)
            ))
        };
        let candidates: Vec<SocketAddr> = match address {
            Some(ip) => vec![SocketAddr::new(ip, port)],
            None => (host, port)
                .to_socket_addrs()
                .map(Iterator::collect)
                .map_err(|_| failed("host not found"))?,
        };
        match connect_any(&candidates, CONNECT_TIMEOUT, CONNECT_TOTAL_TIMEOUT) {
            Ok(socket) => NetStream::new(socket),
            Err(Some(err)) => Err(failed(describe(&err))),
            Err(None) => Err(failed("host not found")),
        }
    }

    fn new(socket: TcpStream) -> io::Result<NetStream> {
        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        socket.set_write_timeout(Some(READ_TIMEOUT))?;
        Ok(NetStream {
            socket,
            closed: Cell::new(false),
        })
    }

    /// Send all of `bytes`.
    pub fn send_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.check_open()?;
        (&self.socket)
            .write_all(bytes)
            .and_then(|()| (&self.socket).flush())
            .map_err(|err| failure("send", &err))
    }

    /// The next bytes that arrive, at most `max` of them, waiting at most
    /// [`READ_TIMEOUT`]. No bytes means the other side ended the stream.
    pub fn receive(&self, max: usize) -> io::Result<Vec<u8>> {
        self.check_open()?;
        let mut buffer = vec![0u8; max];
        let read = (&self.socket)
            .read(&mut buffer)
            .map_err(|err| failure("receive", &err))?;
        buffer.truncate(read);
        Ok(buffer)
    }

    /// End the connection now. Every later use fails.
    pub fn close(&self) {
        if !self.closed.replace(true) {
            // A socket already reset by the other side has nothing left to
            // shut down, and no one to tell.
            let _ = self.socket.shutdown(Shutdown::Both);
        }
    }

    fn check_open(&self) -> io::Result<()> {
        if self.closed.get() {
            return Err(io::Error::other("the connection is closed"));
        }
        Ok(())
    }
}

/// Send `message` as one UDP datagram to the address literal `host` at
/// `port` (1 to 65535), and wait at most [`DATAGRAM_TIMEOUT`] for one
/// datagram back, which is returned. The socket is connected, so a datagram
/// from any other address or port is never delivered to it, and the system
/// chooses the source port at random. A datagram whose first two bytes
/// (a DNS transaction id) are not the message's own is skipped. `host` must be an IPv4 or IPv6
/// literal: this resolves no name (the Oska stub resolver is what asks
/// name servers, by address).
pub fn exchange_datagram(host: &str, port: i128, message: &[u8]) -> io::Result<Vec<u8>> {
    let port = check_port(port, 1)?;
    let Some(ip) = parse_host(host)? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid host {host:?}: a datagram goes to an IP address"),
        ));
    };
    if message.len() > MAX_DATAGRAM {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the datagram is too long to send",
        ));
    }
    let failed =
        |why: &str| io::Error::other(format!("cannot reach {}: {why}", shown_address(host, port)));
    let local = match ip {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let socket = UdpSocket::bind(local).map_err(|err| failed(describe(&err)))?;
    socket
        .connect(SocketAddr::new(ip, port))
        .map_err(|err| failed(describe(&err)))?;
    socket
        .set_read_timeout(Some(DATAGRAM_TIMEOUT))
        .map_err(|err| failed(describe(&err)))?;
    socket.send(message).map_err(|err| failed(describe(&err)))?;
    // The first datagram whose first two bytes, a DNS transaction id, are
    // the message's own; others (a late answer to an earlier query, a
    // forged one) are skipped until the deadline.
    let deadline = Instant::now() + DATAGRAM_TIMEOUT;
    let mut buffer = vec![0u8; MAX_DATAGRAM];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(failed("timed out"));
        }
        socket
            .set_read_timeout(Some(left))
            .map_err(|err| failed(describe(&err)))?;
        let read = socket
            .recv(&mut buffer)
            .map_err(|err| failed(describe(&err)))?;
        if read >= 2 && message.len() >= 2 && buffer[..2] == message[..2] {
            buffer.truncate(read);
            return Ok(buffer);
        }
    }
}

/// The time one more address may take: `per_address`, cut down to what is
/// left of `total` after `spent`, or `None` when none is left.
fn attempt_budget(spent: Duration, per_address: Duration, total: Duration) -> Option<Duration> {
    let left = total.checked_sub(spent).filter(|left| !left.is_zero())?;
    Some(per_address.min(left))
}

/// Connect to the first of `candidates` that answers, each for at most
/// `per_address`, and all of them for at most `total`. The error is the last
/// failure, `timed out` once the total is spent, or `None` for no address.
fn connect_any(
    candidates: &[SocketAddr],
    per_address: Duration,
    total: Duration,
) -> Result<TcpStream, Option<io::Error>> {
    let started = Instant::now();
    let mut last = None;
    for candidate in candidates {
        let Some(budget) = attempt_budget(started.elapsed(), per_address, total) else {
            return Err(Some(io::Error::from(io::ErrorKind::TimedOut)));
        };
        match TcpStream::connect_timeout(candidate, budget) {
            Ok(socket) => return Ok(socket),
            Err(err) => last = Some(err),
        }
    }
    Err(last)
}

/// A listening socket on the loopback address, `127.0.0.1` or `::1`, never
/// on one that other machines can reach.
#[derive(Debug)]
pub struct NetListener {
    socket: TcpListener,
}

impl NetListener {
    /// Listen on the loopback address at `port`, 0 to 65535: 0 chooses a
    /// free one. `127.0.0.1` is tried first, then `::1`.
    pub fn bind_loopback(port: i128) -> io::Result<NetListener> {
        let port = check_port(port, 0)?;
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .or_else(|_| TcpListener::bind((Ipv6Addr::LOCALHOST, port)))
            .map_err(|err| failure("listen", &err))?;
        socket.set_nonblocking(true)?;
        Ok(NetListener { socket })
    }

    /// The port it listens on.
    pub fn port(&self) -> io::Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    /// The next connection, waiting at most [`READ_TIMEOUT`] for one.
    pub fn accept(&self) -> io::Result<NetStream> {
        let started = Instant::now();
        loop {
            match self.socket.accept() {
                Ok((socket, _)) => {
                    socket.set_nonblocking(false)?;
                    return NetStream::new(socket);
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    if started.elapsed() >= READ_TIMEOUT {
                        return Err(failure("accept", &err));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(err) => return Err(failure("accept", &err)),
            }
        }
    }
}

/// The IPv4 literals of the DNS servers a `FIXED_INFO` lists (what
/// `GetNetworkParams` writes on Windows x64), in order. `buffer` holds the
/// structure and the nodes that follow it, and `base` is the address the
/// buffer was at when it was filled, since the nodes link by address. A
/// link that leaves the buffer ends the list, and a node that is not an
/// IPv4 literal is skipped, so a damaged buffer gives fewer servers and
/// never a fault.
///
/// `FIXED_INFO` has `HostName[132]` and `DomainName[132]`, then
/// `CurrentDnsServer` (a pointer, at 264), then `DnsServerList` at 272: an
/// `IP_ADDR_STRING`, which is `Next` (a pointer), `IpAddress[16]`,
/// `IpMask[16]` and `Context` (a `DWORD`), 48 bytes.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn servers_from_fixed_info(buffer: &[u8], base: usize) -> Vec<String> {
    const LIST: usize = 272;
    const NODE: usize = 48;
    let mut servers = Vec::new();
    let mut at = LIST;
    for _ in 0..16 {
        let Some(node) = buffer.get(at..at + NODE) else {
            break;
        };
        let address = &node[8..24];
        let end = address.iter().position(|b| *b == 0).unwrap_or(16);
        if let Some(ip) = std::str::from_utf8(&address[..end])
            .ok()
            .and_then(|text| text.parse::<Ipv4Addr>().ok())
        {
            servers.push(ip.to_string());
        }
        let next = usize::from_le_bytes(node[..8].try_into().unwrap_or([0; 8]));
        match next.checked_sub(base) {
            Some(offset) if next != 0 => at = offset,
            _ => break,
        }
    }
    servers
}

/// `port` as the port number it names, or why it names none: a port is
/// `lowest` (0 for a listener, which lets the system choose; 1 for a
/// connection) to 65535.
fn check_port(port: i128, lowest: u16) -> io::Result<u16> {
    u16::try_from(port)
        .ok()
        .filter(|port| *port >= lowest)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("port {port} is out of range: use {lowest} to 65535"),
            )
        })
}

/// The address `host` is, when it is an IP literal, or `None` for a DNS
/// name; an error for text that is neither.
fn parse_host(host: &str) -> io::Result<Option<IpAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(Some(ip));
    }
    if is_dns_name(host) {
        return Ok(None);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid host {host:?}: it is not a DNS name or an IP address"),
    ))
}

/// Whether `host` is a DNS name: dot-separated labels of letters, digits,
/// `-` and `_`, none empty or too long, with an optional final dot.
fn is_dns_name(host: &str) -> bool {
    let name = host.strip_suffix('.').unwrap_or(host);
    !name.is_empty()
        && name.len() <= MAX_HOST_LEN
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= MAX_LABEL_LEN
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// `host:port`, with an IPv6 literal in brackets.
fn shown_address(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The error for a failed `action` on an open socket, in Oska's wording.
fn failure(action: &str, err: &io::Error) -> io::Error {
    io::Error::other(format!("{action} failed: {}", describe(err)))
}

/// Oska's wording for why a network operation failed: the same on every
/// platform and for every system error. Unreachable is told from refused
/// by the system's code, since `io::ErrorKind` has no stable kind for it
/// on the oldest Rust Oska builds with.
pub fn describe(err: &io::Error) -> &'static str {
    use io::ErrorKind::{
        BrokenPipe, ConnectionAborted, ConnectionRefused, ConnectionReset, TimedOut, WouldBlock,
    };
    match err.kind() {
        ConnectionRefused => "connection refused",
        ConnectionReset | ConnectionAborted | BrokenPipe => "connection reset",
        TimedOut | WouldBlock => "timed out",
        _ => match err.raw_os_error() {
            Some(code) if NETWORK_UNREACHABLE.contains(&code) => "network unreachable",
            Some(code) if HOST_UNREACHABLE.contains(&code) => "host unreachable",
            _ => crate::OTHER_WORDING,
        },
    }
}

/// `ENETUNREACH` on Linux and macOS, `WSAENETUNREACH` on Windows.
const NETWORK_UNREACHABLE: &[i32] = &[101, 51, 10051];

/// `EHOSTUNREACH` on Linux and macOS, `WSAEHOSTUNREACH` on Windows.
const HOST_UNREACHABLE: &[i32] = &[113, 65, 10065];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_a_dns_name_or_an_ip_address() {
        for good in [
            "example.com",
            "a-b.c_d.example.",
            "localhost",
            "127.0.0.1",
            "::1",
        ] {
            assert!(parse_host(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "a b",
            "a/b",
            "exa mple.com",
            "..",
            "a..b",
            "ex@mple.com",
            "http://x",
        ] {
            let err = parse_host(bad).expect_err("an error");
            assert!(err.to_string().contains("invalid host"), "{bad}: {err}");
        }
    }

    /// A `FIXED_INFO` buffer at `base` listing `servers`, the first node
    /// inline and the rest after the structure, as Windows lays it out.
    fn fixed_info(base: usize, servers: &[&str]) -> Vec<u8> {
        let mut buffer = vec![0u8; 272 + 48 * servers.len().max(1)];
        for (i, server) in servers.iter().enumerate() {
            let at = 272 + 48 * i;
            let next = if i + 1 < servers.len() {
                base + 272 + 48 * (i + 1)
            } else {
                0
            };
            buffer[at..at + 8].copy_from_slice(&next.to_le_bytes());
            buffer[at + 8..at + 8 + server.len()].copy_from_slice(server.as_bytes());
        }
        buffer
    }

    #[test]
    fn the_servers_of_a_fixed_info_come_in_order() {
        let buffer = fixed_info(0x1000, &["192.168.1.1", "8.8.8.8", "1.1.1.1"]);
        assert_eq!(
            servers_from_fixed_info(&buffer, 0x1000),
            ["192.168.1.1", "8.8.8.8", "1.1.1.1"]
        );
        assert!(servers_from_fixed_info(&fixed_info(0x1000, &[]), 0x1000).is_empty());
    }

    #[test]
    fn a_damaged_fixed_info_gives_fewer_servers_and_no_fault() {
        let mut buffer = fixed_info(0x1000, &["10.0.0.1", "10.0.0.2"]);
        // The first node's link points outside the buffer.
        buffer[272..280].copy_from_slice(&0x9000usize.to_le_bytes());
        assert_eq!(servers_from_fixed_info(&buffer, 0x1000), ["10.0.0.1"]);
        // The link points before the buffer, and the buffer is cut short.
        buffer[272..280].copy_from_slice(&0x10usize.to_le_bytes());
        assert_eq!(servers_from_fixed_info(&buffer, 0x1000), ["10.0.0.1"]);
        assert!(servers_from_fixed_info(&buffer[..100], 0x1000).is_empty());
        // A link back to the first node would loop for ever.
        buffer[272..280].copy_from_slice(&0x1000usize.wrapping_add(272).to_le_bytes());
        assert_eq!(servers_from_fixed_info(&buffer, 0x1000).len(), 16);
    }

    #[test]
    fn a_datagram_exchange_gets_its_answer_from_the_server_only() {
        let server = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let port = i128::from(server.local_addr().expect("address").port());
        let echo = std::thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (read, peer) = server.recv_from(&mut buffer).expect("receive");
            // Another socket answers first: it must be ignored. So must an
            // answer from the right socket with another id.
            let stranger = UdpSocket::bind("127.0.0.1:0").expect("bind");
            stranger.send_to(b"quiet", peer).expect("forge");
            server.send_to(b"other", peer).expect("wrong id");
            server.send_to(&buffer[..read], peer).expect("answer");
        });
        let answer = exchange_datagram("127.0.0.1", port, b"query").expect("exchange");
        assert_eq!(answer, b"query");
        echo.join().expect("server");
    }

    #[test]
    fn a_datagram_goes_to_an_ip_address_and_a_good_port() {
        let err = exchange_datagram("example.com", 53, b"x").expect_err("a name");
        assert!(err.to_string().contains("goes to an IP address"), "{err}");
        let err = exchange_datagram("127.0.0.1", 0, b"x").expect_err("port");
        assert_eq!(err.to_string(), "port 0 is out of range: use 1 to 65535");
    }

    #[test]
    fn a_port_is_1_to_65535() {
        assert_eq!(check_port(1, 1).ok(), Some(1));
        assert_eq!(check_port(65535, 1).ok(), Some(65535));
        for bad in [0, -1, 65536] {
            let err = check_port(bad, 1).expect_err("an error");
            assert_eq!(
                err.to_string(),
                format!("port {bad} is out of range: use 1 to 65535")
            );
        }
    }

    #[test]
    fn every_address_gets_ten_seconds_and_all_of_them_thirty() {
        let second = Duration::from_secs;
        assert_eq!(CONNECT_TIMEOUT, second(10));
        assert_eq!(CONNECT_TOTAL_TIMEOUT, second(30));
        let budget = |spent| attempt_budget(second(spent), CONNECT_TIMEOUT, CONNECT_TOTAL_TIMEOUT);
        assert_eq!(budget(0), Some(second(10)));
        assert_eq!(budget(25), Some(second(5)));
        assert_eq!(budget(30), None);
        assert_eq!(budget(31), None);
    }

    /// The value a runtime function of `rt.osk` returns, when its body is one
    /// integer.
    fn runtime_constant(name: &str) -> u128 {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../oska-check/src/std/rt.osk");
        let source = std::fs::read_to_string(path).expect("rt.osk");
        let head = format!("fn {name}() -> Int {{\n    ");
        let at = source.find(&head).expect("the function") + head.len();
        let digits: String = source[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().expect("an integer")
    }

    #[test]
    fn the_native_runtime_waits_as_long_as_the_interpreter() {
        assert_eq!(
            runtime_constant("connect_wait"),
            CONNECT_TIMEOUT.as_millis()
        );
        assert_eq!(
            runtime_constant("connect_total_wait"),
            CONNECT_TOTAL_TIMEOUT.as_millis()
        );
        assert_eq!(runtime_constant("read_wait"), READ_TIMEOUT.as_millis());
    }

    #[test]
    fn a_spent_total_stops_the_next_address_with_timed_out() {
        let listener = NetListener::bind_loopback(0).expect("listen");
        let port = listener.port().expect("port");
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let err = connect_any(&[address], CONNECT_TIMEOUT, Duration::ZERO).expect_err("no time");
        assert_eq!(err.map(|e| describe(&e)), Some("timed out"));
        assert!(connect_any(&[address], CONNECT_TIMEOUT, CONNECT_TOTAL_TIMEOUT).is_ok());
    }

    #[test]
    fn a_loopback_round_trip_and_a_refused_connection() {
        let listener = NetListener::bind_loopback(0).expect("listen");
        let port = i128::from(listener.port().expect("port"));
        let client = NetStream::connect("127.0.0.1", port).expect("connect");
        let server = listener.accept().expect("accept");
        client.send_all(b"ping").expect("send");
        assert_eq!(server.receive(16).expect("receive"), b"ping");
        client.close();
        assert_eq!(server.receive(16).expect("end"), b"");
        assert!(client.send_all(b"x").is_err());
        drop(server);
        drop(listener);
        let err = NetStream::connect("127.0.0.1", port).expect_err("an error");
        assert_eq!(
            err.to_string(),
            format!("cannot connect to 127.0.0.1:{port}: connection refused")
        );
    }
}
