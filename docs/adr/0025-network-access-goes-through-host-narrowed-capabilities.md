# Network access goes through host-narrowed capabilities, and `Net` only attenuates

- **Status:** Accepted (2026-11-22), for spec #228 (#237). Mirrors [ADR 0008](0008-file-access-goes-through-confined-capabilities.md) for the network.

## Decision

`Net`, `NetConnect` and `NetListen` grant no connection themselves. Their only methods are attenuations, as `Fs`'s are ([ADR 0008](0008-file-access-goes-through-confined-capabilities.md)):

- `Net.restrict_to(host, port)` and `NetConnect.restrict_to(host, port)` return a **host capability**, `HostConnect`, confined to one host and one port. It alone can connect: `HostConnect.connect()` takes no destination. `NetConnect.connect` does not exist. `Net.connect_only()` and `Net.listen_only()` remain the way to `NetConnect` and `NetListen`.
- `NetListen.restrict_to_loopback(port)` returns a `LoopbackListen`, which accepts connections on the loopback address only. It binds `127.0.0.1`, and `::1` only where that fails, so it listens on one of the two, not both. Port 0 chooses a free port, and `port()` reports it. Listening is loopback-only in this milestone, and the type name says so, so no program becomes a network-facing server by accident. It exists for the local test server.

Code that really reaches a host chosen at run time validates it and writes a trust assertion, which `oskac audit` lists.

The host passed to `restrict_to` is a trusted `String`, so it is a structural position: untrusted text there is `E0300`, as in a first filesystem root. It is a DNS name, an IPv4 literal or an IPv6 literal. Attenuation never fails. A host that is none of those, or a port outside 1 to 65535, makes `connect()` fail with an `IoError` that says why, so a bad address is a reported error and never a panic.

Everything read from a connection is `Untrusted`. `connect()` resolves the name and tries each address in turn; there is no separate DNS API. Failures (host not found, refused, timed out, unreachable, reset) have the same `IoError` message on every platform, from Oska's own wording, never the system's text.

In tests, `t.net` is a stand-in `Net`. In a plain test it refuses every connection and listen with one stable `IoError` and records the host and port asked for (`t.net_attempts()`), so SSRF tests can assert where code tried to go. Under `--integration` it is the real network.

## Why

Before this decision `NetConnect` could reach every host on the internet. A function's signature could not say "this function talks only to `oskalang.dev`", and a package's authority could not be narrower than "the whole network". That is the shape ADR 0008 removed from the filesystem. The self-updater of the install research already assumes a signature that declares `NetConnect` attenuated to `oskalang.dev`. TLS (spec #228) ties the name it sends and checks to the host the capability is confined to, so no code path can verify one name and connect to another. That needs the host to be in the capability.

## Considered options

- **Keep `NetConnect.connect(host, port)` and track trust on the host.** It leaves two ways to connect and no way for a signature to name a host. Rejected for the reasons ADR 0008 rejected the same shape for files.
- **A single `Net` that confines at run time only.** The checker could not see it, and the authority report could not list it. Rejected.

## Consequences

- **Programs that reach any host say so.** They validate the host and write `.assume_trusted()`, which is visible in the code and in `oskac audit`.
- **Every narrowing is in the authority report**, as an attenuation with its host and port (`NetConnect -> HostConnect: restrict_to "oskalang.dev:443"`), and a loopback listener with its port.
- **Package authority has an order.** `Net` > `NetConnect` > `HostConnect`, and `Net` > `NetListen` > `LoopbackListen`. Moving up that order is a widening.
- **`main` cannot declare `HostConnect` or `LoopbackListen`** (`E0217`): only attenuation makes one.
- **`E0370` no longer refuses `Net`, `NetConnect`, `NetListen` or `Connection`.** Under the parity gate ([ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)) they run on both backends: the interpreter first, then the native backend (ticket #238 of spec #228) with sockets in `std.rt`, whose name resolution is `std.dns`, as the interpreter's is (ticket #691). The resolver runs with the runtime's own capabilities, not narrowed by the program's `restrict_to`: see ADR 0026.
- **This is a breaking change to the prelude, with no deprecation period.** `NetConnect.connect` goes; the examples, the guides and the eval tasks move to `restrict_to(host, port).connect()` in the same change.
