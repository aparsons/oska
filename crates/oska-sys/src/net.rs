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
//! on the operating system. Name resolution is the system's for now: the
//! Oska stub resolver is a later ticket of spec #228.

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::time::{Duration, Instant};

/// How long `connect` waits for each address to answer.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a read, or an `accept`, waits for the other side.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

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
        let port = check_port(port)?;
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
        let mut last = None;
        for candidate in candidates {
            match TcpStream::connect_timeout(&candidate, CONNECT_TIMEOUT) {
                Ok(socket) => return NetStream::new(socket),
                Err(err) => last = Some(err),
            }
        }
        Err(match last {
            Some(err) => failed(describe(&err)),
            None => failed("host not found"),
        })
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
        let port = u16::try_from(port).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("port {port} is out of range: use 0 to 65535"),
            )
        })?;
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

/// `port` as the port number it names, or why it names none.
fn check_port(port: i128) -> io::Result<u16> {
    u16::try_from(port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("port {port} is out of range: use 1 to 65535"),
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
            let err = parse_host(bad).unwrap_err();
            assert!(err.to_string().contains("invalid host"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_port_is_1_to_65535() {
        assert_eq!(check_port(1).ok(), Some(1));
        assert_eq!(check_port(65535).ok(), Some(65535));
        for bad in [0, -1, 65536] {
            let err = check_port(bad).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("port {bad} is out of range: use 1 to 65535")
            );
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
        let err = NetStream::connect("127.0.0.1", port).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("cannot connect to 127.0.0.1:{port}: connection refused")
        );
    }
}
