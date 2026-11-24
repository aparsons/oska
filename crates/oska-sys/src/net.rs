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
//! on the operating system. This module resolves no name: the caller
//! resolves one with `std.dns`, the Oska stub resolver (ADR 0026), and
//! connects to the addresses. [`exchange_datagram`] is the UDP half that
//! `std.dns` asks name servers with.

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket,
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
    /// Connect to `host` at `port`. `host` is an IPv4 or IPv6 literal and
    /// `port` is 1 to 65535; anything else is an error that says so, and no
    /// socket is opened. A DNS name is never resolved here, by the system's
    /// resolver or any other: the caller resolves it (`std.dns`) and passes
    /// the addresses to [`NetStream::connect_resolved`], so a name given
    /// here is `host not found`.
    pub fn connect(host: &str, port: i128) -> io::Result<NetStream> {
        NetStream::connect_resolved(host, &[], port)
    }

    /// Connect to `host` at `port` through `addresses`, the address
    /// literals its name resolved to, each tried in turn for at most
    /// [`CONNECT_TIMEOUT`] and all of them for at most
    /// [`CONNECT_TOTAL_TIMEOUT`]. For an IP literal `host`, `addresses` is
    /// ignored and the literal is the one address. `host` is only what the
    /// errors name.
    pub fn connect_resolved(host: &str, addresses: &[String], port: i128) -> io::Result<NetStream> {
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
            None => addresses
                .iter()
                .filter_map(|text| text.parse::<IpAddr>().ok())
                .map(|ip| SocketAddr::new(ip, port))
                .collect(),
        };
        match connect_any(&candidates, CONNECT_TIMEOUT, CONNECT_TOTAL_TIMEOUT) {
            Ok(socket) => NetStream::new(socket),
            Err(Some(err)) => Err(failed(describe(&err))),
            Err(None) => Err(failed("host not found")),
        }
    }

    /// Whether `host` is a DNS name that needs resolving, rather than an IP
    /// literal; an error for text that is neither, as [`NetStream::connect`]
    /// words it. The port is checked too, so a bad one is refused before
    /// any name server is asked.
    pub fn needs_resolving(host: &str, port: i128) -> io::Result<bool> {
        check_port(port, 1)?;
        Ok(parse_host(host)?.is_none())
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
/// from any other address or port is never delivered to it, and the source
/// port is random, from 49152 to 65535. A datagram whose first two bytes
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
    let any = match ip {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    let socket = bind_random_port(any).map_err(|err| failed(describe(&err)))?;
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

/// The first port of the dynamic range, and how many ports it holds
/// (49152 to 65535, a power of two so a random number maps evenly).
const DYNAMIC_PORTS: u32 = 16384;
const DYNAMIC_FIRST: u32 = 49152;

/// How many random ports to try before giving up on a busy range.
const BIND_TRIES: usize = 16;

/// A UDP socket bound to `address` at a random port of the dynamic range,
/// drawn from the system's secure random source, so an off-path attacker
/// cannot guess where the answer must go (spec #228). A port in use is
/// replaced by another draw.
fn bind_random_port(address: IpAddr) -> io::Result<UdpSocket> {
    let mut last = io::Error::new(io::ErrorKind::AddrInUse, "no free port");
    for _ in 0..BIND_TRIES {
        let mut word = [0u8; 4];
        crate::fill_random(&mut word)?;
        let port = DYNAMIC_FIRST + u32::from_le_bytes(word) % DYNAMIC_PORTS;
        let port = u16::try_from(port).unwrap_or(u16::MAX);
        match UdpSocket::bind(SocketAddr::new(address, port)) {
            Ok(socket) => return Ok(socket),
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => last = err,
            Err(err) => return Err(err),
        }
    }
    Err(last)
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

/// A listener on the loopback addresses, `127.0.0.1` and `::1` when the
/// machine has both, at one port; never on an address that other machines can
/// reach. A machine without an IPv6 loopback listens on `127.0.0.1` alone.
#[derive(Debug)]
pub struct NetListener {
    sockets: Vec<TcpListener>,
}

impl NetListener {
    /// Listen on the loopback addresses at `port`, 0 to 65535: 0 chooses a
    /// free one. `127.0.0.1` is bound first, then `::1` at the same port. A
    /// failure to bind one of them leaves the other, and only when both fail
    /// is it an error.
    pub fn bind_loopback(port: i128) -> io::Result<NetListener> {
        let port = check_port(port, 0)?;
        // A chosen port may be free on one family and taken on the other, so
        // ask again for another (a few times) rather than listen on one.
        let tries = if port == 0 { 8 } else { 1 };
        let mut sockets = Vec::new();
        let mut last = None;
        for _ in 0..tries {
            sockets.clear();
            let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, port));
            let at = match &v4 {
                Ok(socket) => socket.local_addr()?.port(),
                Err(_) => port,
            };
            let v6 = TcpListener::bind((Ipv6Addr::LOCALHOST, at));
            match (v4, v6) {
                (Ok(a), Ok(b)) => {
                    sockets.extend([a, b]);
                    break;
                }
                (Ok(a), Err(err)) if port != 0 || !port_taken(&err) => {
                    sockets.push(a);
                    break;
                }
                (Err(_), Ok(b)) => {
                    sockets.push(b);
                    break;
                }
                (Ok(_), Err(err)) | (Err(_), Err(err)) => last = Some(err),
            }
        }
        if sockets.is_empty() {
            let err = last.unwrap_or_else(|| io::Error::from(io::ErrorKind::AddrInUse));
            return Err(failure("listen", &err));
        }
        for socket in &sockets {
            socket.set_nonblocking(true)?;
        }
        Ok(NetListener { sockets })
    }

    /// The port it listens on.
    pub fn port(&self) -> io::Result<u16> {
        Ok(self.sockets[0].local_addr()?.port())
    }

    /// The next connection, on either address, waiting at most
    /// [`READ_TIMEOUT`] for one.
    pub fn accept(&self) -> io::Result<NetStream> {
        let started = Instant::now();
        loop {
            for socket in &self.sockets {
                match socket.accept() {
                    Ok((socket, _)) => {
                        socket.set_nonblocking(false)?;
                        return NetStream::new(socket);
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                    Err(err) => return Err(failure("accept", &err)),
                }
            }
            if started.elapsed() >= READ_TIMEOUT {
                return Err(failure(
                    "accept",
                    &io::Error::from(io::ErrorKind::WouldBlock),
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Whether `err` says the address is in use.
fn port_taken(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::AddrInUse
}

/// The literals of the DNS servers of the adapters that are up, from the
/// `IP_ADAPTER_ADDRESSES` list `GetAdaptersAddresses` writes on Windows
/// x64, in the order Windows asks them: by the adapter's interface metric
/// for the server's family (lowest first), and in list order among equal
/// metrics. A server is listed once, at its lowest metric.
///
/// `buffer` holds the structures and everything they point to, and `base`
/// is the address the buffer was at when it was filled, since the
/// structures link by address. A link that leaves the buffer ends its
/// list, and an entry that cannot be read is skipped, so a damaged buffer
/// gives fewer servers and never a fault.
///
/// Left out: an adapter that is not `IfOperStatusUp`, a loopback adapter,
/// an unspecified address, and an IPv6 address that is link-local
/// (`fe80::/10`, which needs the interface's scope, which a literal cannot
/// carry) or site-local (`fec0::/10`, deprecated, which Windows lists as
/// placeholders on an adapter with no IPv6 name server). IPv6 literals are
/// written as eight groups of four hex digits, as `std.rt` writes them.
///
/// Offsets, for x64: `IP_ADAPTER_ADDRESSES` has `Next` at 8,
/// `FirstDnsServerAddress` at 48, `IfType` at 100, `OperStatus` at 104,
/// `Ipv4Metric` at 216 and `Ipv6Metric` at 220, 224 bytes in all up to
/// there. `IP_ADAPTER_DNS_SERVER_ADDRESS` has `Next` at 8, then a
/// `SOCKET_ADDRESS` (`lpSockaddr` at 16, `iSockaddrLength` at 24), 32
/// bytes.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn servers_from_adapter_addresses(buffer: &[u8], base: usize) -> Vec<String> {
    const ADAPTER: usize = 224;
    const NODE: usize = 32;
    const IF_OPER_STATUS_UP: u32 = 1;
    const IF_TYPE_LOOPBACK: u32 = 24;
    let word = |at: usize| -> Option<usize> {
        Some(usize::from_le_bytes(
            buffer.get(at..at + 8)?.try_into().ok()?,
        ))
    };
    let half = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(buffer.get(at..at + 4)?.try_into().ok()?))
    };
    // The offset in `buffer` of the structure at address `link`.
    // Never past the buffer, so the arithmetic on it cannot overflow.
    let offset = |link: usize| {
        link.checked_sub(base)
            .filter(|at| link != 0 && *at <= buffer.len())
    };
    let mut found: Vec<(u32, String)> = Vec::new();
    let mut adapter = Some(0usize);
    let mut adapters = 0;
    while let Some(at) = adapter.take().filter(|_| adapters < 128) {
        adapters += 1;
        let (Some(status), Some(kind), Some(v4), Some(v6)) = (
            half(at + 104),
            half(at + 100),
            half(at + 216),
            half(at + 220),
        ) else {
            break;
        };
        if at + ADAPTER <= buffer.len() && status == IF_OPER_STATUS_UP && kind != IF_TYPE_LOOPBACK {
            let mut node = word(at + 48).and_then(offset);
            let mut count = 0;
            while let Some(n) = node.take().filter(|_| count < 16) {
                count += 1;
                if let Some((family, text)) = server_literal(buffer, offset, n, NODE) {
                    let metric = if family == 2 { v4 } else { v6 };
                    found.push((metric, text));
                }
                node = word(n + 8).and_then(offset);
            }
        }
        adapter = word(at + 8).and_then(offset);
    }
    // Stable: equal metrics keep the order of the adapters.
    found.sort_by_key(|(metric, _)| *metric);
    // A server listed twice keeps its place at the lowest metric.
    let mut servers: Vec<String> = Vec::new();
    for (_, text) in found {
        if !servers.contains(&text) {
            servers.push(text);
        }
    }
    servers
}

/// The family and literal of the server in the `IP_ADAPTER_DNS_SERVER_ADDRESS`
/// at `at`, or `None` when its address cannot be read or is left out.
fn server_literal(
    buffer: &[u8],
    offset: impl Fn(usize) -> Option<usize>,
    at: usize,
    size: usize,
) -> Option<(u16, String)> {
    let node = buffer.get(at..at + size)?;
    let sockaddr = offset(usize::from_le_bytes(node[16..24].try_into().ok()?))?;
    let length = i32::from_le_bytes(node[24..28].try_into().ok()?);
    let family = u16::from_le_bytes(buffer.get(sockaddr..sockaddr + 2)?.try_into().ok()?);
    match family {
        2 if length >= 16 => {
            let octets: [u8; 4] = buffer.get(sockaddr + 4..sockaddr + 8)?.try_into().ok()?;
            let ip = Ipv4Addr::from(octets);
            (!ip.is_unspecified()).then(|| (2, ip.to_string()))
        }
        23 if length >= 28 => {
            let octets: [u8; 16] = buffer.get(sockaddr + 8..sockaddr + 24)?.try_into().ok()?;
            let ip = Ipv6Addr::from(octets);
            let top = ip.segments()[0] & 0xffc0;
            if ip.is_unspecified() || top == 0xfe80 || top == 0xfec0 {
                return None;
            }
            let groups: Vec<String> = ip.segments().iter().map(|g| format!("{g:04x}")).collect();
            Some((23, groups.join(":")))
        }
        _ => None,
    }
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
            _ => crate::OTHER_WORDING,
        },
    }
}

/// `ENETUNREACH` and `EHOSTUNREACH` on Linux and macOS, `WSAENETUNREACH` and
/// `WSAEHOSTUNREACH` on Windows. The spec's error table (#228) has one
/// wording for both, `network unreachable`.
const NETWORK_UNREACHABLE: &[i32] = &[101, 113, 51, 65, 10051, 10065];

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

    /// One adapter for `adapters_buffer`: its link status, type, metrics
    /// and DNS servers (`4` or `6` and the address text).
    struct Adapter {
        up: bool,
        kind: u32,
        metrics: (u32, u32),
        servers: Vec<(u8, &'static str)>,
    }

    fn adapter(up: bool, metric: u32, servers: &[(u8, &'static str)]) -> Adapter {
        Adapter {
            up,
            kind: 6,
            metrics: (metric, metric),
            servers: servers.to_vec(),
        }
    }

    /// A buffer at `base` laid out as `GetAdaptersAddresses` does: every
    /// adapter 224 bytes, linked by address, then the 32-byte server nodes
    /// and their socket addresses.
    fn adapters_buffer(base: usize, adapters: &[Adapter]) -> Vec<u8> {
        let mut buffer = vec![0u8; 224 * adapters.len().max(1)];
        let mut tail = buffer.len();
        for (i, a) in adapters.iter().enumerate() {
            let at = 224 * i;
            if i + 1 < adapters.len() {
                buffer[at + 8..at + 16].copy_from_slice(&(base + at + 224).to_le_bytes());
            }
            buffer[at + 100..at + 104].copy_from_slice(&a.kind.to_le_bytes());
            let status: u32 = if a.up { 1 } else { 2 };
            buffer[at + 104..at + 108].copy_from_slice(&status.to_le_bytes());
            buffer[at + 216..at + 220].copy_from_slice(&a.metrics.0.to_le_bytes());
            buffer[at + 220..at + 224].copy_from_slice(&a.metrics.1.to_le_bytes());
            let mut previous: Option<usize> = None;
            for (family, text) in &a.servers {
                let node = tail;
                let sockaddr = node + 32;
                let (bytes, length) = if *family == 4 {
                    let ip: Ipv4Addr = text.parse().expect("an IPv4 literal");
                    let mut s = vec![0u8; 16];
                    s[..2].copy_from_slice(&2u16.to_le_bytes());
                    s[4..8].copy_from_slice(&ip.octets());
                    (s, 16i32)
                } else {
                    let ip: Ipv6Addr = text.parse().expect("an IPv6 literal");
                    let mut s = vec![0u8; 32];
                    s[..2].copy_from_slice(&23u16.to_le_bytes());
                    s[8..24].copy_from_slice(&ip.octets());
                    (s, 28)
                };
                buffer.resize(sockaddr + bytes.len(), 0);
                buffer[sockaddr..].copy_from_slice(&bytes);
                buffer[node..node + 4].copy_from_slice(&32u32.to_le_bytes());
                buffer[node + 16..node + 24].copy_from_slice(&(base + sockaddr).to_le_bytes());
                buffer[node + 24..node + 28].copy_from_slice(&length.to_le_bytes());
                match previous {
                    None => buffer[at + 48..at + 56].copy_from_slice(&(base + node).to_le_bytes()),
                    Some(p) => buffer[p + 8..p + 16].copy_from_slice(&(base + node).to_le_bytes()),
                }
                previous = Some(node);
                tail = buffer.len();
            }
        }
        buffer
    }

    const BASE: usize = 0x10_0000;

    #[test]
    fn the_servers_of_an_adapter_come_in_order() {
        let buffer = adapters_buffer(
            BASE,
            &[adapter(
                true,
                25,
                &[(4, "192.168.1.1"), (4, "8.8.8.8"), (4, "1.1.1.1")],
            )],
        );
        assert_eq!(
            servers_from_adapter_addresses(&buffer, BASE),
            ["192.168.1.1", "8.8.8.8", "1.1.1.1"]
        );
        assert!(servers_from_adapter_addresses(&adapters_buffer(BASE, &[]), BASE).is_empty());
    }

    #[test]
    fn a_disconnected_adapters_servers_are_not_listed() {
        let buffer = adapters_buffer(
            BASE,
            &[
                adapter(false, 5, &[(4, "10.9.9.9")]),
                adapter(true, 25, &[(4, "192.168.1.1")]),
            ],
        );
        assert_eq!(
            servers_from_adapter_addresses(&buffer, BASE),
            ["192.168.1.1"]
        );
    }

    #[test]
    fn servers_come_in_metric_order_and_equal_metrics_keep_list_order() {
        let buffer = adapters_buffer(
            BASE,
            &[
                adapter(true, 50, &[(4, "10.0.0.50")]),
                adapter(true, 10, &[(4, "10.0.0.10"), (4, "10.0.0.11")]),
                adapter(true, 50, &[(4, "10.0.0.51")]),
                adapter(true, 5, &[(4, "10.0.0.50")]),
            ],
        );
        // The duplicate keeps its place at the lowest metric (5).
        assert_eq!(
            servers_from_adapter_addresses(&buffer, BASE),
            ["10.0.0.50", "10.0.0.10", "10.0.0.11", "10.0.0.51"]
        );
    }

    #[test]
    fn a_servers_metric_is_its_familys() {
        let mut wifi = adapter(true, 0, &[(4, "10.0.0.2"), (6, "2001:db8::2")]);
        wifi.metrics = (40, 10);
        let mut wired = adapter(true, 0, &[(4, "10.0.0.1")]);
        wired.metrics = (20, 99);
        let buffer = adapters_buffer(BASE, &[wifi, wired]);
        assert_eq!(
            servers_from_adapter_addresses(&buffer, BASE),
            [
                "2001:0db8:0000:0000:0000:0000:0000:0002",
                "10.0.0.1",
                "10.0.0.2"
            ]
        );
    }

    #[test]
    fn ipv6_servers_are_listed_but_link_local_site_local_and_loopback_are_not() {
        let mut loopback = adapter(true, 1, &[(4, "10.1.1.1")]);
        loopback.kind = 24;
        let buffer = adapters_buffer(
            BASE,
            &[
                loopback,
                adapter(
                    true,
                    10,
                    &[
                        (6, "fe80::1"),
                        (6, "fec0:0:0:ffff::1"),
                        (6, "::"),
                        (4, "0.0.0.0"),
                        (6, "2606:4700:4700::1111"),
                        (6, "fd00::53"),
                    ],
                ),
            ],
        );
        assert_eq!(
            servers_from_adapter_addresses(&buffer, BASE),
            [
                "2606:4700:4700:0000:0000:0000:0000:1111",
                "fd00:0000:0000:0000:0000:0000:0000:0053"
            ]
        );
    }

    #[test]
    fn a_damaged_adapter_list_gives_fewer_servers_and_no_fault() {
        let mut buffer = adapters_buffer(
            BASE,
            &[
                adapter(true, 10, &[(4, "10.0.0.1"), (4, "10.0.0.2")]),
                adapter(true, 20, &[(4, "10.0.0.3")]),
            ],
        );
        // The first server's link points outside the buffer.
        let first = usize::from_le_bytes(buffer[48..56].try_into().expect("8 bytes")) - BASE;
        buffer[first + 8..first + 16].copy_from_slice(&0x9000_0000usize.to_le_bytes());
        // The second adapter's link points before the buffer.
        buffer[8..16].copy_from_slice(&0x10usize.to_le_bytes());
        assert_eq!(servers_from_adapter_addresses(&buffer, BASE), ["10.0.0.1"]);
        // The buffer is cut short.
        assert!(servers_from_adapter_addresses(&buffer[..100], BASE).is_empty());
        // An adapter that links to itself ends, and lists its servers once.
        buffer[8..16].copy_from_slice(&BASE.to_le_bytes());
        assert_eq!(servers_from_adapter_addresses(&buffer, BASE), ["10.0.0.1"]);
        // A server that links to itself ends the list too.
        let again = BASE + first;
        buffer[first + 8..first + 16].copy_from_slice(&again.to_le_bytes());
        assert_eq!(servers_from_adapter_addresses(&buffer, BASE), ["10.0.0.1"]);
    }

    #[test]
    fn a_link_far_past_the_buffer_does_not_overflow() {
        // A base of 1 makes the offset of a link near `usize::MAX` huge.
        let mut buffer = adapters_buffer(1, &[adapter(true, 10, &[(4, "10.0.0.1")])]);
        assert_eq!(servers_from_adapter_addresses(&buffer, 1), ["10.0.0.1"]);
        let first = usize::from_le_bytes(buffer[48..56].try_into().expect("8 bytes")) - 1;
        for link in [usize::MAX, usize::MAX - 50, usize::MAX - 100] {
            buffer[8..16].copy_from_slice(&link.to_le_bytes());
            buffer[first + 8..first + 16].copy_from_slice(&link.to_le_bytes());
            assert_eq!(servers_from_adapter_addresses(&buffer, 1), ["10.0.0.1"]);
        }
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
    fn queries_leave_from_random_ports_in_the_dynamic_range() {
        let server = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let port = i128::from(server.local_addr().expect("address").port());
        let echo = std::thread::spawn(move || {
            let mut seen = Vec::new();
            let mut buffer = [0u8; 64];
            for _ in 0..8 {
                let (read, peer) = server.recv_from(&mut buffer).expect("receive");
                seen.push(peer.port());
                server.send_to(&buffer[..read], peer).expect("answer");
            }
            seen
        });
        for _ in 0..8 {
            exchange_datagram("127.0.0.1", port, b"query").expect("exchange");
        }
        let seen = echo.join().expect("server");
        assert!(seen.iter().all(|port| *port >= 49152), "{seen:?}");
        assert!(seen.iter().any(|port| *port != seen[0]), "{seen:?}");
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

    /// The spec's error table (#228) has `network unreachable` and no
    /// `host unreachable`: a host the system cannot route to reads the same.
    #[test]
    fn every_unreachable_code_reads_network_unreachable() {
        for code in [101, 113, 51, 65, 10051, 10065] {
            let err = io::Error::from_raw_os_error(code);
            assert_eq!(describe(&err), "network unreachable", "code {code}");
        }
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
    fn a_listener_accepts_on_both_loopback_addresses_the_machine_has() {
        let listener = NetListener::bind_loopback(0).expect("listen");
        let port = i128::from(listener.port().expect("port"));
        let mut hosts = vec!["127.0.0.1"];
        if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
            hosts.push("::1");
        }
        for host in hosts {
            let client = NetStream::connect(host, port).expect(host);
            let server = listener.accept().expect(host);
            client.send_all(b"hi").expect("send");
            assert_eq!(server.receive(16).expect("receive"), b"hi", "{host}");
        }
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
