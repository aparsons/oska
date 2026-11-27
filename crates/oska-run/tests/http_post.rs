// SPDX-License-Identifier: Apache-2.0
//! `std.http` `POST` (#831 of spec #824) on the interpreter, against the
//! loopback HTTPS test server of #244 (`crates/oskac/tests/tls/server.osk`,
//! an Oska program, with the test CA of `tls/pki/`). The server runs on a
//! thread of its own through `oska_run::run`, and the client program
//! (`tls/http_client.osk`) runs on this one, both on real loopback sockets.
//! The same scenarios run natively, through the differential harness, in
//! `crates/oskac/tests/native_http.rs`.
//!
//! What is held here: a `POST` reaches the server with its body,
//! `Content-Type` and `Content-Length`; the headers of the response
//! (`Replay-Nonce`, `Location`, `Retry-After`, `Link`) are read by name in
//! any case; an oversized request body, too many request headers and a
//! response head past its limit are each refused with an error.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};

use oska_run::{run, Host, Output, RealHost, RootHandle};

const SERVER: &str = include_str!("../../oskac/tests/tls/server.osk");
const CLIENT: &str = include_str!("../../oskac/tests/tls/http_client.osk");
const ROOT: &str = include_str!("../../oskac/tests/tls/pki/root.hex");

fn pki(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oskac/tests/tls/pki")
        .join(file)
}

/// [`RealHost`] with its stdout sent down a channel as it is written, so
/// the test can read the port the server prints while the server runs.
/// Every other operation goes straight to the real OS.
struct Channelled {
    real: RealHost,
    stdout: Sender<String>,
}

impl Host for Channelled {
    fn write_stdout(&mut self, text: &str) -> std::io::Result<()> {
        let _ = self.stdout.send(text.to_string());
        Ok(())
    }
    fn write_stderr(&mut self, text: &str) -> std::io::Result<()> {
        let _ = std::io::stderr().write_all(text.as_bytes());
        Ok(())
    }
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        Ok(None)
    }
    fn env_var(&mut self, name: &str) -> Option<String> {
        self.real.env_var(name)
    }
    fn now_millis(&mut self) -> i128 {
        self.real.now_millis()
    }
    fn next_random(&mut self) -> u64 {
        self.real.next_random()
    }
    fn secure_bytes(&mut self, len: usize) -> std::io::Result<Vec<u8>> {
        self.real.secure_bytes(len)
    }
    fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.real.open_root(path)
    }
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<RootHandle> {
        self.real.open_beneath(root, names)
    }
    fn open_file_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.real.open_file_root(path)
    }
    fn open_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<RootHandle> {
        self.real.open_file_beneath(root, names)
    }
    fn read_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<Vec<u8>> {
        self.real.read_file_beneath(root, names)
    }
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> std::io::Result<()> {
        self.real.write_file_beneath(root, names, contents)
    }
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<()> {
        self.real.create_dir_beneath(root, names)
    }
    fn list_dir_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<Vec<String>> {
        self.real.list_dir_beneath(root, names)
    }
    fn net_connect(&mut self, host: &str, port: i128) -> std::io::Result<oska_sys::NetStream> {
        self.real.net_connect(host, port)
    }
    fn net_listen_loopback(&mut self, port: i128) -> std::io::Result<oska_sys::NetListener> {
        self.real.net_listen_loopback(port)
    }
    fn printed(&self) -> Vec<String> {
        Vec::new()
    }
    fn take_output(&mut self) -> Output {
        Output::default()
    }
}

/// What the client printed for `mode` (and the arguments after it) against
/// the server's `scenario`, which serves `connections` connections.
fn client_says(scenario: &str, mode: &str, extra: &[&str], connections: usize) -> String {
    let (port_tx, port_rx) = channel();
    let server = (connections > 0).then(|| {
        let args: Vec<String> = [
            "server.osk".to_string(),
            pki("chain_good.hex").display().to_string(),
            pki("leaf.key.hex").display().to_string(),
            scenario.to_string(),
            connections.to_string(),
        ]
        .into();
        std::thread::spawn(move || {
            let mut host = Channelled {
                real: RealHost::new(),
                stdout: port_tx,
            };
            let outcome = run(SERVER, &mut host, &args);
            assert!(
                outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
                "the server should run: {outcome:#?}"
            );
        })
    });
    let port = if server.is_some() {
        port_rx
            .recv()
            .expect("the server prints its port")
            .trim()
            .to_string()
    } else {
        "65000".to_string()
    };
    let (out_tx, out_rx) = channel();
    let mut args: Vec<String> = vec![
        "http_client.osk".to_string(),
        port.clone(),
        "127.0.0.1".to_string(),
        mode.to_string(),
        ROOT.trim().to_string(),
    ];
    args.extend(extra.iter().map(ToString::to_string));
    let mut host = Channelled {
        real: RealHost::new(),
        stdout: out_tx,
    };
    let outcome = run(CLIENT, &mut host, &args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "the client should run: {outcome:#?}"
    );
    drop(host);
    if let Some(server) = server {
        server.join().expect("the server ends cleanly");
    }
    out_rx.try_iter().collect::<String>().replace(&port, "PORT")
}

#[test]
fn a_post_reaches_the_server_with_its_body_content_type_and_length() {
    assert_eq!(
        client_says("http_echo", "post", &[], 1),
        "status 200\nx-test none\nlocation none\nnonce none\nretry none\nlink none\n\
         body [POST /order HTTP/1.1\r\nHost: 127.0.0.1:PORT\r\nConnection: close\r\n\
         Accept-Encoding: identity\r\nUser-Agent: oska\r\n\
         Content-Type: application/jose+json\r\nContent-Length: 7\r\n\r\n{\"a\":1}]\n"
    );
}

#[test]
fn a_posts_body_is_sent_as_given_whatever_it_holds() {
    let said = client_says("http_echo", "post", &["a\r\n\r\nGET /x HTTP/1.1"], 1);
    assert!(
        said.contains("Content-Length: 20\r\n\r\na\r\n\r\nGET /x HTTP/1.1]"),
        "{said:?}"
    );
}

#[test]
fn response_headers_are_read_by_name_in_any_case() {
    assert_eq!(
        client_says("http_nonce", "post", &[], 1),
        "status 201\nx-test none\nlocation https://ca.test/acct/1\nnonce n-1\nretry 3\n\
         link <https://ca.test/dir>;rel=\"index\"\nbody [ok]\n"
    );
}

#[test]
fn a_request_body_over_the_maximum_is_refused_before_connecting() {
    assert_eq!(
        client_says("http_ok", "post_big", &[], 0),
        "error invalid request: the request body is over 16777216\n"
    );
}

#[test]
fn more_than_100_request_headers_are_refused_before_connecting() {
    assert_eq!(
        client_says("http_ok", "post_headers", &[], 0),
        "error invalid request: the request has more than 100 headers\n"
    );
}

#[test]
fn a_response_head_over_the_limit_is_refused() {
    assert_eq!(
        client_says("http_big_head", "post", &[], 1),
        "error the response is too large\n"
    );
    assert_eq!(
        client_says("http_many_headers", "post", &[], 1),
        "error the response is too large\n"
    );
}

#[test]
fn a_post_may_not_set_its_own_content_type_and_get_refuses_a_post() {
    assert_eq!(
        client_says("http_ok", "post_content_type", &[], 0),
        "error invalid request: the header content-type is set by the client\n"
    );
    assert_eq!(
        client_says("http_ok", "post_via_get", &[], 0),
        "error invalid request: http.get sends a GET request\n"
    );
}
