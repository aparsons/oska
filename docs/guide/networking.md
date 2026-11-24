# Networking with host capabilities

A program reaches the network through a **host capability**: a `HostConnect` that can connect to one host and one port, and to nothing else. `Net` and `NetConnect` give no connection themselves. They only make a narrower capability. This is the same rule that `Dir` and `FileRead` follow for files ([ADR 0025](../adr/0025-network-access-goes-through-host-narrowed-capabilities.md), after [ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)). Every program on this page is checked by `oskac` when the documentation is tested.

## One host, one port

`net.restrict_to(host, port)` gives a `HostConnect`. Its `connect()` takes no host and no port, so a function that holds one cannot be pointed at another place. `name()` and `port()` say where it goes.

```oska
fn fetch_banner(host: HostConnect) -> Result<Int, IoError> {
    let connection = host.connect()?
    let banner = connection.receive()?
    Ok(banner.len())
}

fn main(io: Stdout, net: Net, args: Args) -> Result<(), IoError> {
    if !args.get(1).is_some() {
        return io.println("pass any argument to connect")
    }
    let host = net.restrict_to("mail.example.com", 25)
    let name = host.name()
    let size = fetch_banner(host)?
    io.println("read {size} bytes from {name}")
}
```

The signature of `fetch_banner` is its promise: it talks to the one host it was given. `oskac check` prints the authority report, which lists the host and the port, as it lists a file root:

```console cmd=check
fn fetch_banner: HostConnect (host)
fn main: Stdout (io), Net (net)
  main.osk:11:16: Net -> HostConnect: restrict_to "mail.example.com:25" (bytes 339..378)
```

## The host is a structural position

The `host` argument of `restrict_to` decides where the program connects, so it must be trusted text. A string you wrote in the source is trusted. Text from a file, the command line or a network reply is not, and passing it is `E0300`. This stops an attacker from choosing the destination (server-side request forgery).

A program that must reach a host chosen at run time checks the host first, and then writes `.assume_trusted()`. That call is the one place a reviewer must read, and `oskac audit` lists it.

## What connect does

`connect()` returns a `Connection` or an `IoError`. A host is a DNS name in ASCII, an IPv4 address or an IPv6 address without brackets. A bad host or a port outside 1 to 65535 is an `IoError` that says why, not a panic. Every failure has the same message on every platform: `host not found`, `connection refused`, `timed out`, `network unreachable`, `connection reset` or `connection closed`. No operating-system text reaches the message.

A connection waits at most 10 seconds for each address and 30 seconds in all, and a read waits at most 60 seconds for the next byte.

Everything a connection reads is untrusted. `receive()` gives `Untrusted<String>` and fails on invalid UTF-8. `receive_bytes()` gives `Untrusted<Bytes>`: what is there, up to 64 KiB, and an empty `Bytes` at the end of the stream. `send(text)` and `send_bytes(bytes)` write. `host()` gives the name the capability was confined to, and `close()` closes the connection.

Both backends look a name up with `std.dns`, the Oska stub resolver, reading the hosts file and `resolv.conf`. `std.dns` is also a module a program can call itself. `HostConnect.exchange_datagram` and `NetConnect.system_name_servers`, the two methods `std.dns` uses, run on both backends.

## Listening, for a local test server

`net.listen_only()` gives a `NetListen`, and `restrict_to_loopback(port)` on it gives a `LoopbackListen`. It accepts connections from this machine only: it listens on `127.0.0.1`, or on `::1` where that address is not available, not on both at once. Port 0 picks a free port, and `port()` says which. Nothing in Oska can listen on a public address yet.

```oska
fn serve_once(listen: LoopbackListen) -> Result<(), IoError> {
    let connection = listen.accept()?
    connection.send("hello\n")?
    connection.close()
    Ok(())
}

fn main(io: Stdout, net: Net, args: Args) -> Result<(), IoError> {
    if !args.get(1).is_some() {
        return io.println("pass any argument to serve")
    }
    let listen = net.listen_only().restrict_to_loopback(0)
    io.println("serving on 127.0.0.1")?
    serve_once(listen)
}
```

```console cmd=check
fn serve_once: LoopbackListen (listen)
fn main: Stdout (io), Net (net)
  main.osk:12:18: Net -> NetListen: listen_only (bytes 348..365)
  main.osk:12:18: NetListen -> LoopbackListen: restrict_to_loopback "loopback:0" (bytes 348..389)
```

## Testing

In a plain `test`, `t.net` is a stand-in `Net`. It refuses every connect and listen with one stable `IoError`, and `t.net_attempts()` lists the host and port that code tried to reach (the design text for this feature called it `net_requests`; the name in the code, and so in your programs, is `net_attempts`), so a test can check where code goes without a network. Under `--integration`, `t.net` is the real network. See [testing](testing.md).

## Authority

Package authority orders the kinds: `Net` is wider than `NetConnect`, and `NetConnect` is wider than `HostConnect`. `Net` is also wider than `NetListen`, which is wider than `LoopbackListen`. A dependency that moves to a wider kind is a widening that needs approval. Which host a capability names is the caller's choice, so it is not part of a package's authority, but the report lists it. See [capabilities](capabilities.md).

For encrypted connections, see [TLS](tls.md). For a file over HTTPS, see [HTTP](http.md).
