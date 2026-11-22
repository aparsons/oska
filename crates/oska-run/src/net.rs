//! The network intrinsics: attenuating `Net`, `NetConnect` and `NetListen`
//! to a host capability or a loopback listener, and what those and a
//! `Connection` do (ADR 0023).
//!
//! `Net`, `NetConnect` and `NetListen` only attenuate. A connection is
//! made by `HostConnect.connect`, which takes no destination, and a
//! listener by `NetListen.restrict_to_loopback`. Both go through the
//! [`Host`], so a test's host can refuse them and a real one reaches
//! `oska-sys`.

use std::io;
use std::rc::Rc;

use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::{io_bytes, io_failure, io_text, io_unit, Args, Intrinsic::Plain, Table};
use crate::memory;
use crate::panics::Panic;
use crate::value::{Capability, HostCap, LoopbackCap, NetCap, StreamHandle, Value};

/// The most bytes one `receive` returns.
const CHUNK: usize = 64 * 1024;

pub const INTRINSICS: Table = &[
    ("Connection.close", Plain(connection_close)),
    ("Connection.host", Plain(connection_host)),
    ("Connection.receive", Plain(connection_receive)),
    ("Connection.receive_bytes", Plain(connection_receive_bytes)),
    ("Connection.send", Plain(connection_send)),
    ("Connection.send_bytes", Plain(connection_send_bytes)),
    ("HostConnect.connect", Plain(host_connect)),
    ("HostConnect.name", Plain(host_name)),
    ("HostConnect.port", Plain(host_port)),
    ("LoopbackListen.accept", Plain(loopback_accept)),
    ("LoopbackListen.port", Plain(loopback_port)),
    ("Net.connect_only", Plain(connect_only)),
    ("Net.listen_only", Plain(listen_only)),
    ("Net.restrict_to", Plain(restrict_to)),
    ("NetConnect.restrict_to", Plain(restrict_to)),
    (
        "NetListen.restrict_to_loopback",
        Plain(restrict_to_loopback),
    ),
];

/// `Net.connect_only(self)`.
fn connect_only(_: &mut dyn Host, _: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Capability(Capability::Net(NetCap::Connect)))
}

/// `Net.listen_only(self)`.
fn listen_only(_: &mut dyn Host, _: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::Capability(Capability::Net(NetCap::Listen)))
}

/// `restrict_to(self, host, port)` on `Net` and `NetConnect`: the host
/// capability for the one host and port. Attenuation never fails: a host
/// or port that cannot be connected to fails at `connect`, with the reason.
fn restrict_to(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let Value::Str(host) = &args[1] else {
        unreachable!("the checker guarantees a string host, found {:?}", args[1]);
    };
    let Value::Int(port, _) = &args[2] else {
        unreachable!(
            "the checker guarantees an integer port, found {:?}",
            args[2]
        );
    };
    Ok(Value::Capability(Capability::Host(HostCap {
        host: Rc::from(&**host),
        port: *port,
        stream: None,
    })))
}

/// `restrict_to_loopback(self, port)` on `NetListen`: the listener, which
/// the host opens now. A port that cannot be listened on leaves a listener
/// whose `accept` fails with the reason.
fn restrict_to_loopback(host: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let Value::Int(port, _) = &args[1] else {
        unreachable!(
            "the checker guarantees an integer port, found {:?}",
            args[1]
        );
    };
    let listener = host
        .net_listen_loopback(*port)
        .map(Rc::new)
        .map_err(|err| Rc::<str>::from(oska_sys::io_error_message(&err)));
    Ok(Value::Capability(Capability::Loopback(LoopbackCap(
        listener,
    ))))
}

fn host_cap(value: &Value) -> &HostCap {
    match value {
        Value::Capability(Capability::Host(host)) => host,
        other => unreachable!("the checker guarantees a host capability, found {other:?}"),
    }
}

/// A `Connection` to `host`, holding `stream`.
fn connection(host: &HostCap, stream: oska_sys::NetStream, span: Span) -> Result<Value, Panic> {
    let held = Capability::Host(HostCap {
        stream: Some(StreamHandle(Rc::new(stream))),
        ..host.clone()
    });
    memory::structure("Connection", vec![Value::Capability(held)], span)
}

/// `HostConnect.connect(self)`.
fn host_connect(host: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let cap = host_cap(&args[0]).clone();
    match host.net_connect(&cap.host, cap.port) {
        Ok(stream) => Ok(Value::ok(connection(&cap, stream, span)?)),
        Err(err) => io_failure(&err, span),
    }
}

/// `HostConnect.name(self)`.
fn host_name(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::text(&host_cap(&args[0]).host, span)
}

/// `HostConnect.port(self)`.
fn host_port(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(Value::int(host_cap(&args[0]).port))
}

/// The host a `Connection` (`value`) holds, and its stream.
fn held(value: &Value) -> (&HostCap, &oska_sys::NetStream) {
    let host = host_cap(&value.fields()[0]);
    match &host.stream {
        Some(stream) => (host, &stream.0),
        None => unreachable!("a `Connection` always holds its stream"),
    }
}

/// `Connection.host(self)`.
fn connection_host(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    memory::text(&held(&args[0]).0.host, span)
}

/// `Connection.close(self)`.
fn connection_close(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    held(&args[0]).1.close();
    Ok(Value::Unit)
}

/// `Connection.send(self, body)`.
fn connection_send(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let Value::Str(body) = &args[1] else {
        unreachable!("the checker guarantees text, found {:?}", args[1]);
    };
    io_unit(held(&args[0]).1.send_all(body.as_bytes()), span)
}

/// `Connection.send_bytes(self, body)`.
fn connection_send_bytes(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_unit(held(&args[0]).1.send_all(args[1].as_bytes()), span)
}

/// `Connection.receive(self)`.
fn connection_receive(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_text(held(&args[0]).1.receive(CHUNK), span)
}

/// `Connection.receive_bytes(self)`.
fn connection_receive_bytes(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    io_bytes(held(&args[0]).1.receive(CHUNK), span)
}

fn loopback(value: &Value) -> &LoopbackCap {
    match value {
        Value::Capability(Capability::Loopback(listener)) => listener,
        other => unreachable!("the checker guarantees a loopback listener, found {other:?}"),
    }
}

/// `LoopbackListen.accept(self)`: the next connection, as a `Connection`
/// to `loopback` at the listener's port.
fn loopback_accept(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let listener = match &loopback(&args[0]).0 {
        Ok(listener) => listener,
        Err(why) => return io_failure(&io::Error::other(why.to_string()), span),
    };
    let port = listener.port().map_or(0, i128::from);
    match listener.accept() {
        Ok(stream) => {
            let peer = HostCap {
                host: Rc::from("loopback"),
                port,
                stream: None,
            };
            Ok(Value::ok(connection(&peer, stream, span)?))
        }
        Err(err) => io_failure(&err, span),
    }
}

/// `LoopbackListen.port(self)`: the port, or 0 when it could not listen.
fn loopback_port(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    let port = match &loopback(&args[0]).0 {
        Ok(listener) => listener.port().map_or(0, i128::from),
        Err(_) => 0,
    };
    Ok(Value::int(port))
}
