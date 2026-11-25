// SPDX-License-Identifier: Apache-2.0
//! Name resolution for `HostConnect.connect()` on the interpreter: the
//! Oska stub resolver, `std.dns` (ADR 0026), run on the real host, never
//! the operating system's resolver.
//!
//! `std.dns` is written in Oska, so it runs as a small program of its own
//! on a nested [`Interpreter`] that shares the host. The hosts file and
//! `resolv.conf` are read by the resolver's own loader (`dns.load_config`)
//! unless a [`Files`] gives them as text, which a test uses to point the
//! resolver at a name server of its own.

use std::cell::RefCell;
use std::io;

use oska_check::program::Program;
use oska_syntax::Span;

use crate::host::Host;
use crate::interp::Interpreter;
use crate::value::{Capability, Value};

/// The hosts file and `resolv.conf` as text, and the port the name servers
/// listen on, in place of the files of this machine and port 53.
#[derive(Debug, Clone)]
pub struct Files {
    pub hosts: String,
    pub resolv_conf: String,
    pub port: i128,
}

const SOURCE: &str = "use std.dns

pub fn system(fs: FsRead, net: NetConnect, random: Random, env: Env, name: String) -> Result<List<String>, String> {
    dns.resolve_system(fs, net, random, env, name)
}

pub fn given(net: NetConnect, random: Random, hosts: String, resolv_conf: String, port: Int, name: String) -> Result<List<String>, String> {
    let config = dns.Config.new(hosts, resolv_conf, [])
    match dns.resolve(config, dns.NetExchange.on_port(net, random, port), name) {
        Ok(found) => Ok(found)
        Err(error) => Err(error.to_string())
    }
}
";

thread_local! {
    static PROGRAM: RefCell<Option<std::rc::Rc<Program>>> = const { RefCell::new(None) };
}

fn program() -> std::rc::Rc<Program> {
    PROGRAM.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                let report = oska_check::check_module(SOURCE);
                std::rc::Rc::new(report.program.unwrap_or_else(|| {
                    unreachable!("the resolver checks: {:?}", report.diagnostics)
                }))
            })
            .clone()
    })
}

/// The addresses of the DNS name `name`, as `std.dns` resolves it on
/// `host`, or the resolver's own wording of why not (`host not found`,
/// `timed out`, ...).
pub(crate) fn resolve(
    host: &mut dyn Host,
    files: Option<&Files>,
    name: &str,
) -> io::Result<Vec<String>> {
    let program = program();
    let cap = |ty: &str| {
        Value::Capability(Capability::granted(ty).unwrap_or_else(|| unreachable!("{ty}")))
    };
    let (function, args) = match files {
        None => (
            "system",
            vec![
                cap("FsRead"),
                cap("NetConnect"),
                cap("Random"),
                cap("Env"),
                Value::str(name),
            ],
        ),
        Some(files) => (
            "given",
            vec![
                cap("NetConnect"),
                cap("Random"),
                Value::str(&files.hosts),
                Value::str(&files.resolv_conf),
                Value::int(files.port),
                Value::str(name),
            ],
        ),
    };
    let main = program
        .functions
        .get(function)
        .unwrap_or_else(|| unreachable!("the resolver has `{function}`"));
    let mut interpreter = Interpreter::new(&program, host);
    let value = interpreter
        .call(main, args, Span::at(0))
        .map_err(|panic| io::Error::other(format!("name resolution failed: {panic:?}")))?;
    let inner = value.fields()[0].clone();
    if value.is_variant(oska_check::builtins::ERR) {
        return Err(io::Error::other(inner.as_str().to_string()));
    }
    Ok(inner
        .as_list()
        .iter()
        .map(|address| address.as_str().to_string())
        .collect())
}
