# TLS 1.3

`std.tls` is a TLS 1.3 client written in Oska ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)). `tls.connect(host, config, random, clock)` takes a `HostConnect` (see [networking](networking.md)), runs the handshake and gives back a `TlsStream`. The code is hand-written and has not been audited outside the project. It is held by a loopback test server written in Oska, by recorded handshakes and by the vectors of its crypto modules ([hashing and signatures](hashing-and-signatures.md)). Every program on this page is checked by `oskac` when the documentation is tested.

## Connect and talk

```oska
use std.tls

fn ping(host: HostConnect, random: Random, clock: Clock) -> Result<Int, tls.TlsError> {
    var stream = tls.connect(host, tls.TlsConfig.default(), random, clock)?
    stream.send("ping".to_bytes())?
    let reply = stream.receive()?
    stream.close()
    Ok(reply.len())
}

fn main(io: Stdout, net: Net, random: Random, clock: Clock) -> Result<(), IoError> {
    match ping(net.restrict_to("oskalang.dev", 443), random, clock) {
        Ok(count) => io.println("reply of {count} bytes")
        Err(error) => io.println("failed: {error}")
    }
}
```

The program takes `Random` and `Clock` as arguments, because a handshake needs secure random bytes and the time to check certificates. `send` takes bytes, `receive` gives `Untrusted<Bytes>`, and `close` sends `close_notify`.

## Which name is checked

The name in the server name indication and in the certificate check is always `host.name()`. There is no argument to change it, so no code path can check one name and connect to another. Certificates are checked by [`std.x509`](x509.md): the chain against Mozilla's pinned roots, the validity dates at `clock`'s time, and the host name by RFC 9525.

To trust other roots, build a configuration. `tls.TlsConfig.default().with_trust(trust)` takes an `x509.TrustConfig`. Roots read from bytes are trusted bytes, so adding one is an audit site, as the x509 page shows. `with_alpn(protocols)` offers application protocols.

## What the client does

- It offers TLS 1.3 only, and refuses a server that answers with an older version or sets the downgrade marker.
- It offers `TLS_CHACHA20_POLY1305_SHA256` and `TLS_AES_128_GCM_SHA256`, and key exchange with x25519 (and P-256 after a `HelloRetryRequest`).
- It checks the CertificateVerify signature (Ed25519, ECDSA P-256 and P-384, RSA-PSS), the Finished message in constant time, and every record's tag before it uses the plaintext.
- It bounds the size of records, handshake messages and the certificate chain.
- It ignores `NewSessionTicket`, handles `KeyUpdate`, and sends the alert RFC 8446 names before it fails.
- A stream that ends without `close_notify` is `Truncated`, so a cut-off reply is never taken for a whole one.

## Errors

`TlsError` has stable kinds: `Certificate` (with the `X509Error`), `AlertReceived` (with the alert's name), `Protocol`, `Unsupported`, `Truncated` and `Io` (with the reason, such as `i/o error: connection refused`). Match on the kind, not on the message.

## The engine, for tests

`tls.Engine` is the handshake and record layer without a socket. It takes bytes, 64 bytes of secure randomness and the time, and gives bytes to send and events. Use it to test protocol code without a network, or over another transport.

## Trust

`std.tls` has no `.assume_trusted()`. It calls `expose()` only for the public key share, the client random and session ID, a record's nonce, the ciphertext to send and the plaintext after its tag verifies, and a test holds that list. See [secrets](secrets.md) and the [threat model](threat-model.md).
