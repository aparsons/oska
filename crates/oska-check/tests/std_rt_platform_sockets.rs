// SPDX-License-Identifier: Apache-2.0
//! The socket values in `std.rt`'s platform tables (ticket #238 of spec
//! #228): macOS's and Windows' (Linux's are checked in
//! `std_rt_platform_linux.rs`), so a wrong entry fails here and not only
//! on the platform's runner.

mod common;

use common::platform::{fields_of, owned};

const SOCKET_FIELDS: [&str; 16] = [
    "af_inet6",
    "sol_socket",
    "so_error",
    "so_reuseaddr",
    "poll_in",
    "poll_out",
    "poll_timespec",
    "sockaddr_length_byte",
    "e_would_block",
    "e_in_progress",
    "e_refused",
    "e_reset",
    "e_aborted",
    "e_timed_out",
    "e_net_unreach",
    "e_host_unreach",
];

#[test]
fn the_macos_table_holds_xnus_socket_values() {
    let expected = owned(&[
        // sys/socket.h: AF_INET6, SOL_SOCKET, SO_ERROR, and SO_REUSEADDR
        // (sys/socket.h's 0x0004); sys/poll.h: POLLIN and POLLOUT, with
        // `poll` taking milliseconds. A `struct sockaddr` starts with its
        // length in one byte, then its family in one.
        ("af_inet6", 30),
        ("sol_socket", 0xffff),
        ("so_error", 0x1007),
        ("so_reuseaddr", 0x0004),
        ("poll_in", 0x1),
        ("poll_out", 0x4),
        ("poll_timespec", 0),
        ("sockaddr_length_byte", 1),
        // sys/errno.h: EAGAIN, EINPROGRESS, ECONNREFUSED, ECONNRESET,
        // ECONNABORTED, ETIMEDOUT, ENETUNREACH, EHOSTUNREACH.
        ("e_would_block", 35),
        ("e_in_progress", 36),
        ("e_refused", 61),
        ("e_reset", 54),
        ("e_aborted", 53),
        ("e_timed_out", 60),
        ("e_net_unreach", 51),
        ("e_host_unreach", 65),
    ]);
    assert_eq!(fields_of("macos", &SOCKET_FIELDS), expected);
}

#[test]
fn the_windows_table_holds_winsocks_values() {
    let expected = owned(&[
        // winsock2.h and ws2def.h: AF_INET6, SOL_SOCKET, SO_ERROR; no
        // SO_REUSEADDR is set (it would let a second listener take the
        // port); POLLRDNORM and POLLWRNORM, with `WSAPoll` taking
        // milliseconds. A `SOCKADDR` starts with a 16-bit family.
        ("af_inet6", 23),
        ("sol_socket", 0xffff),
        ("so_error", 0x1007),
        ("so_reuseaddr", 0),
        ("poll_in", 0x0100),
        ("poll_out", 0x0010),
        ("poll_timespec", 0),
        ("sockaddr_length_byte", 0),
        // winerror.h: WSAEWOULDBLOCK (which a non-blocking `connect` also
        // gives), WSAECONNREFUSED, WSAECONNRESET, WSAECONNABORTED,
        // WSAETIMEDOUT, WSAENETUNREACH, WSAEHOSTUNREACH.
        ("e_would_block", 10035),
        ("e_in_progress", 10035),
        ("e_refused", 10061),
        ("e_reset", 10054),
        ("e_aborted", 10053),
        ("e_timed_out", 10060),
        ("e_net_unreach", 10051),
        ("e_host_unreach", 10065),
    ]);
    assert_eq!(fields_of("windows_x86_64", &SOCKET_FIELDS), expected);
}
