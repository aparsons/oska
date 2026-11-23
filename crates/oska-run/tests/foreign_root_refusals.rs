// SPDX-License-Identifier: Apache-2.0
//! A root a [`FakeHost`] opened, given to a [`RealHost`] (#359). It holds
//! no OS handle, so the real host refuses every walk beneath it. On
//! Windows the names are checked first (`checked_names` in `host.rs`), so
//! a Windows device name is refused for its name before the root is
//! refused as another host's; elsewhere the device name is an ordinary
//! name and the root's refusal comes first. A plain name meets the
//! root's refusal on every platform.

use oska_run::{FakeHost, Host, RealHost, RootHandle};
use std::path::Path;

const FOREIGN: &str = "this root was opened by another host";

fn fake_root() -> RootHandle {
    FakeHost::new()
        .with_dir("/root")
        .open_root(Path::new("/root"))
        .expect("the fake host opens its own directory")
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

/// The refusal of each walk a [`RealHost`] takes beneath `root` for
/// `walked`, which each walk must refuse.
fn refusals(root: &RootHandle, walked: &[String]) -> Vec<String> {
    let mut host = RealHost::new();
    let message = |result: Result<(), std::io::Error>| match result {
        Ok(()) => panic!("a walk beneath another host's root was not refused"),
        Err(error) => error.to_string(),
    };
    vec![
        message(host.read_file_beneath(root, walked).map(drop)),
        message(host.write_file_beneath(root, walked, b"text")),
        message(host.create_dir_beneath(root, walked)),
        message(host.list_dir_beneath(root, walked).map(drop)),
        message(host.open_beneath(root, walked).map(drop)),
        message(host.open_file_beneath(root, walked).map(drop)),
    ]
}

#[test]
fn a_plain_name_meets_the_foreign_root_refusal() {
    let root = fake_root();
    for refusal in refusals(&root, &names(&["inside.txt"])) {
        assert_eq!(refusal, FOREIGN);
    }
}

#[test]
fn a_device_name_is_refused_for_its_name_first_on_windows() {
    let root = fake_root();
    let expected = match cfg!(windows) {
        true => "`CON` is a Windows device name, and confined access never opens one",
        false => FOREIGN,
    };
    for refusal in refusals(&root, &names(&["sub", "CON"])) {
        assert_eq!(refusal, expected);
    }
}
