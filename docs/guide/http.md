# HTTPS GET

`std.http` fetches a file over HTTPS. `http.get(host, request, tls_config, random, clock)` takes a `HostConnect` (see [networking](networking.md)), speaks HTTP/1.1 over [`std.tls`](tls.md) and returns a `Response`. It is the client that `oskac self check` is built to use for the signed release index. Every program on this page is checked by `oskac` when the documentation is tested.

## Fetch a file

```oska
use std.http
use std.tls

fn fetch(host: HostConnect, random: Random, clock: Clock) -> Result<Int, http.HttpError> {
    let request = http.Request.get("/latest.json")
    let response = http.get(host, request, tls.TlsConfig.default(), random, clock)?
    Ok(response.status)
}

fn main(io: Stdout, net: Net, random: Random, clock: Clock, args: Args) -> Result<(), IoError> {
    if !args.get(1).is_some() {
        return io.println("pass any argument to fetch")
    }
    match fetch(net.restrict_to("oskalang.dev", 443), random, clock) {
        Ok(status) => io.println("status {status}")
        Err(error) => io.println("failed: {error}")
    }
}
```

The request goes to the host the capability names. The `Host` header comes from the capability too, so it always matches where the program connected.

## The response

A `Response` has `status`, `headers` and `body`. `body` is `Untrusted<Bytes>`. `header(name)` finds a header without regard to case and gives `Untrusted<String>`; header names and values are untrusted. Redirects are returned as they are and not followed, so the program never talks to a host it holds no capability for. To follow one, read `Location`, check it and make a new `HostConnect` yourself.

## The request

`http.Request.get(path)` takes untrusted text, so a path from a user is allowed. A path that does not start with `/`, or holds a control character, a space or `#`, fails with `InvalidRequest` before any connection is made. `with_header(name, value)` adds a header. The name is a literal, and the value may be untrusted but may not hold CR, LF or NUL. The client sets `Host`, `Connection` and `Accept-Encoding` itself, and refuses `Content-Length`, `Transfer-Encoding`, `Upgrade`, `Expect` and `TE`.

```oska
use std.http

fn build(path: Untrusted<String>, agent: Untrusted<String>) -> http.Request {
    http.Request.get(path).with_header("User-Agent", agent).with_max_body(1048576)
}

fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    let path = args.get(1).unwrap_or("/")
    let request = build(path, "oska-guide")
    io.println("built")
}
```

## Limits

The body is at most 16 MiB unless `with_max_body(bytes)` says less. A setting over 16 MiB (`http.MAX_BODY`) is `InvalidRequest`, before anything connects. The head of the response is at most 64 KiB and 100 fields. A larger one is `TooLarge`. A connection waits at most 60 seconds for the next bytes.

The reader frames a body by `Content-Length`, by chunked encoding, or by the end of the connection, which it accepts only after `close_notify`. It refuses a response with both lengths, with different lengths, with obsolete line folding or with a bare line feed, so a smuggled response cannot confuse it (`Malformed`).

## Errors

`HttpError` has the kinds `Tls` (with the `TlsError`), `Malformed`, `TooLarge` and `InvalidRequest`. Match on the kind. `http.Reader` is the pure parser, public for tests.
