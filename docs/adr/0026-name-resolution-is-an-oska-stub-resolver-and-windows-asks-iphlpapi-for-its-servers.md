# Name resolution is an Oska stub resolver, and Windows asks `iphlpapi` for its name servers

- **Status:** Accepted (2026-11-22), for spec #228 (#239). Amends [ADR 0020](0020-windows-intrinsics-are-kernel32-and-ntdll-calls.md) for one function, and builds on [ADR 0025](0025-network-access-goes-through-host-narrowed-capabilities.md), whose `connect()` resolves the name.

## Decision

Names are resolved by **`std.dns`, one stub resolver written in Oska**, the same on all five platforms. It reads the hosts file and the name server configuration, asks each name server over UDP, retries over TCP when an answer is truncated, and treats every answer as untrusted data: the transaction id and question must match, compression pointers only point backwards, every length is checked, and only the records that belong to the name asked (or its `CNAME` chain) are kept. `std.dns` calls no `getaddrinfo`. `connect()` does not use it yet, though: see Consequences.

Where the name servers come from:

- **Linux and macOS:** `/etc/resolv.conf` (`nameserver`, `search`, `domain`, and the options `ndots`, `timeout` and `attempts`), and `/etc/hosts`. `std.dns` reads both through `FsRead`.
- **Windows:** the hosts file in `C:\Windows\System32\drivers\etc`, read the same way, and the name servers from **`GetNetworkParams` of `iphlpapi.dll`**, through the intrinsic `rt_sys_get_network_params` and the prelude method `NetConnect.system_name_servers`. `GetNetworkParams` returns the system-wide list in the order Windows uses it, for every adapter, in one call and one buffer. On the other platforms `system_name_servers` gives an empty list.

The datagram itself is one prelude method, `HostConnect.exchange_datagram(message)`: it sends one UDP datagram to the host and port the capability is confined to (an address literal, so no name is resolved on the way) and returns the first datagram from that address whose transaction id is the message's own, waiting at most 5 seconds. TCP uses the existing `connect()`, `send_bytes` and `receive_bytes`.

## Why

- **One behaviour.** The three operating systems' resolvers differ in what they search, in which servers they ask first, in their timeouts and in their wording. [ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md) puts the code that parses attacker-chosen bytes in Oska, and a DNS answer is such a byte string. ADR 0025 says failures read the same on every platform, which a system resolver cannot promise.
- **Why `iphlpapi`, when ADR 0020 allows only `kernel32` and `ntdll`.** The registry holds the servers per interface (`Tcpip\Parameters\Interfaces\{GUID}`, `NameServer` and `DhcpNameServer`), and what Windows actually asks first depends on interface metrics, VPNs and the name resolution policy table, none of which a registry walk sees. Four `ntdll` calls (`NtOpenKey`, `NtEnumerateKey`, `NtQueryValueKey`, `NtClose`) and UTF-16 parsing in `std.rt` would give a list in an order Windows does not use. `GetNetworkParams` is one call, as ADR 0020 asks of an intrinsic, and returns the list the system itself uses. It is the one function of that DLL the runtime imports.
- **IPv4 only.** `GetNetworkParams` lists IPv4 servers only. A machine whose only name servers are IPv6 has none from this call and falls back to `127.0.0.1`. That is a known gap, accepted for a call that needs no structure parsing beyond one linked list; `GetAdaptersAddresses` would close it at the cost of a far larger structure.

## Considered options

- **The system resolver (`getaddrinfo`, `DnsQuery`).** Rejected: it is three behaviours, it links the C library or `dnsapi.dll` into every program, and it parses answers outside Oska.
- **A registry walk through `ntdll`.** Rejected for the reasons above: no new DLL, but a list in the wrong order.
- **A `GetAdaptersAddresses` call.** Rejected for now: IPv6 servers, a much larger structure to walk in `std.rt`, and the same extra DLL.

## Consequences

- **Every Windows executable imports `iphlpapi.dll`.** The import list is always the whole list (ADR 0020), so a Windows program loads one more DLL at start-up, whether or not it resolves names. The import table is checked by `build_windows.rs`, which lists `GetNetworkParams` as the only function of that DLL.
- **Name resolution inside `connect()` is not wired yet.** `connect()` does not use `std.dns`: on the interpreter `HostConnect.connect()` resolves a DNS name with the operating system's resolver (`getaddrinfo`, in `oska-sys`), and the native backend resolves only IP literals and `localhost` (any other name is `host not found`). Moving both to `std.dns` is tracked in a follow-up issue; until then a program that wants Oska's resolver calls `dns.resolve` itself.
- **`HostConnect.exchange_datagram` and `NetConnect.system_name_servers` run on both backends.** The native backend lowers them in `std.rt` through the socket and `GetNetworkParams` intrinsics it already has. `std.dns`'s message, hosts file and `resolv.conf` code is pure Oska and runs on both backends.
