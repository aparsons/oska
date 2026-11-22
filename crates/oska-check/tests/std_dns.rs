//! `std.dns` (#239): the stub resolver in Oska, linked into a program that
//! imports it and checked under the rules for user code. Its names and
//! addresses are structural: an untrusted host name cannot reach `resolve`
//! without a trust assertion, and a configuration read from a file is
//! untrusted text that the parsers check. The module has one audit site:
//! the `expose()` of a query's transaction id, which goes out in the clear.

mod common;

use common::{assert_errors, assert_ok, check};

/// Every public function, called from `main` with a host name from the
/// command line made trusted, and a hosts file and `resolv.conf` read
/// from files.
const EVERY_FUNCTION: &str = r#"use std.dns

fn main(io: Stdout, fs: FsRead, net: NetConnect, random: Random) -> Result<(), IoError> {
    let config = dns.load_config(fs, net)
    let exchange = dns.NetExchange.new(net, random)
    let again = dns.NetExchange.on_port(net, random, 5353)
    match dns.resolve(config, exchange, "example.com") {
        Ok(found) => io.println(" ".join(found))?
        Err(error) => io.println(error.to_string())?
    }
    let hosts = dns.parse_hosts("127.0.0.1 localhost")
    let conf = dns.parse_resolv_conf("nameserver 192.0.2.1")
    let same = dns.Config.new("", "", [])
    let names = dns.lookup_hosts(hosts, "localhost")
    let asked = dns.candidates(same, "www", false)
    let query = dns.build_query(1, "example.com", dns.TYPE_A)
    let literal = dns.parse_ip("::1")
    let valid = dns.valid_name("example.com")
    let known = names.len()
    let searched = asked.len()
    let word = if valid { "valid" } else { "invalid" }
    io.println("{conf.ndots} {known} {searched} {word}")
}
"#;

#[test]
fn a_program_calling_every_function_checks_and_lowers() {
    let report = check(EVERY_FUNCTION);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.program.is_some(), "a clean program lowers");
}

#[test]
fn a_brace_group_imports_its_items_by_name() {
    assert_ok(
        "use std.dns.{parse_ip, valid_name}\n\
         fn ok(text: Untrusted<String>) -> Bool {\n    \
             valid_name(\"a.b\") && parse_ip(text).is_some()\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn ok() -> Bool {\n    dns.valid_name(\"a.b\")\n}",
        &[("E0221", "dns")],
    );
}

#[test]
fn its_helpers_are_private() {
    assert_errors("use std.dns.{read_name}", &[("E0360", "read_name")]);
    assert_errors("use std.dns.{words}", &[("E0360", "words")]);
}

#[test]
fn an_untrusted_host_name_cannot_be_resolved_without_a_trust_assertion() {
    assert_errors(
        "use std.dns\n\
         fn find(config: dns.Config, exchange: dns.NetExchange, name: Untrusted<String>) -> Bool {\n    \
             dns.resolve(config, exchange, name).is_ok()\n}",
        &[("E0300", "name")],
    );
}

#[test]
fn the_files_a_configuration_is_read_from_may_be_untrusted() {
    assert_ok(
        "use std.dns\n\
         fn conf(hosts: Untrusted<String>, resolv: Untrusted<String>) -> dns.Config {\n    \
             dns.Config.new(hosts, resolv, [])\n}",
    );
}

#[test]
fn a_name_server_comes_back_as_trusted_text_a_connection_can_use() {
    assert_ok(
        "use std.dns\n\
         fn first(net: NetConnect, conf: dns.ResolvConf) -> HostConnect {\n    \
             net.restrict_to(conf.servers.at(0), 53)\n}",
    );
}

#[test]
fn the_module_has_one_audit_site_a_public_transaction_id() {
    let report = check(EVERY_FUNCTION);
    let source = include_str!("../src/std/dns.osk");
    for forbidden in [".assume_trusted(", ".expose("] {
        let uses: Vec<&str> = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//") && line.contains(forbidden))
            .collect();
        // `query_id` declassifies the id of a query, which is public: it
        // is sent in the clear.
        if forbidden == ".expose(" {
            assert_eq!(uses.len(), 1, "{uses:?}");
            assert!(uses[0].contains("secure_bytes"), "{uses:?}");
        } else {
            assert!(uses.is_empty(), "{forbidden}: {uses:?}");
        }
    }
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}
