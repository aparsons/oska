// SPDX-License-Identifier: Apache-2.0
//! The test hook of the RFC 8448 replay (#695 of #659) cannot be reached from
//! a program. The replay is `crates/oskac/tests/tls_rfc8448.rs`: it builds a
//! copy of `std.tls` with a hook that fixes the engine's ClientHello to the
//! RFC's. The hook exists only in that copy. The module a program imports has
//! no such name, and a program cannot call one: that is what this file holds.

mod common;

use common::assert_errors;

/// `std.tls` holds no trace of the hook, and a program that names it gets an
/// error for an unknown function and an unknown method.
#[test]
fn the_rfc_8448_hook_is_not_reachable_from_a_program() {
    let module = include_str!("../src/std/tls.osk");
    assert!(
        !module.to_lowercase().contains("rfc8448"),
        "std.tls must not hold the test hook"
    );
    assert_errors(
        "use std.tls\n\
         fn f() -> Int {\n    \
             var e = tls.rfc8448_engine(tls.TlsConfig.default())\n    \
             e.start().output.len()\n}",
        &[("E0312", "rfc8448_engine")],
    );
    assert_errors(
        "use std.tls\n\
         fn f() -> Int {\n    \
             let e = tls.Engine.rfc8448(tls.TlsConfig.default())\n    \
             0\n}",
        &[("E0228", "rfc8448")],
    );
}
