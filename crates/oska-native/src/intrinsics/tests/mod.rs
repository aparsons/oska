// SPDX-License-Identifier: Apache-2.0
//! Tests of the intrinsics.

use super::prelude::PRELUDE;
use super::runtime::RUNTIME;
use super::windows::windows_only;
use super::*;

mod linux;
mod macos;
mod prelude;
mod windows;

#[test]
fn every_std_rt_intrinsic_has_a_native_implementation_and_no_other_does() {
    let declared = oska_check::std_rt_intrinsics();
    let implemented: Vec<&str> = RUNTIME.iter().map(|(name, _)| *name).collect();
    let mut declared_sorted = declared.clone();
    declared_sorted.sort_unstable();
    let mut implemented_sorted = implemented.clone();
    implemented_sorted.sort_unstable();
    assert_eq!(declared_sorted, implemented_sorted);
}

/// `std.rt` builds and reads a `Reservation` as its own `Hold`, so the
/// two must have the same fields, of the same types, in order.
#[test]
fn a_reservation_is_laid_out_as_std_rt_s_hold() {
    use oska_check::program::TypeDeclKind;
    let fields = |program: &oska_check::program::Program, name: &str| match &program
        .types
        .get(name)
        .map(|decl| &decl.kind)
    {
        Some(TypeDeclKind::Struct(fields)) => {
            fields.iter().map(|f| f.ty.to_string()).collect::<Vec<_>>()
        }
        other => panic!("`{name}` is not a struct: {other:?}"),
    };
    let rt = oska_check::check_toolchain_rt()
        .program
        .expect("std.rt should check");
    let user =
        oska_check::check_module("fn main(budget: Budget) {\n    let _ = budget.reserve(1)\n}\n")
            .program
            .expect("the program should check");
    assert_eq!(fields(&rt, "Hold"), ["Int", "Int", "Bool"]);
    assert_eq!(fields(&user, "Reservation"), fields(&rt, "Hold"));
}

#[test]
fn every_platform_table_the_backend_gives_is_a_std_rt_table() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    for platform in crate::Target::ALL.map(crate::Target::platform) {
        let table = program
            .functions
            .get(platform.table)
            .unwrap_or_else(|| panic!("std.rt has no `{}`", platform.table));
        assert!(table.params.is_empty() && !table.receiver);
        assert_eq!(table.ret.to_string(), "Platform");
    }
}

#[test]
fn the_rt_sys_intrinsics_are_the_system_calls() {
    for (name, implementation) in RUNTIME {
        assert_eq!(
            name.starts_with("rt_sys_"),
            matches!(implementation, Runtime::Syscall(_)),
            "`{name}`"
        );
    }
}

/// A program that reaches every system call `std.rt` makes: the
/// standard streams, the clock and randomness, and confined reads,
/// writes, listings and directories, whose paths are resolved against
/// the working directory.
const EVERY_CALL: &str = r#"fn main(io: Stdout, input: Stdin, fs: Fs, clock: Clock, random: Random, net: Net) -> Result<(), IoError> {
    let read = fs.read_only().restrict_to("root")
    let write = fs.write_only().restrict_to("root")
    let names = read.list_dir(Path.new("sub"))?
    let text = read.read_to_string(Path.new("a.txt"))?
    write.write(Path.new("b.txt"), "b")?
    write.create_dir(Path.new("c"))?
    let found = read.exists(Path.new("a.txt"))?
    let meta = read.metadata(Path.new("a.txt"))?
    write.append(Path.new("b.txt"), "b")?
    write.rename(Path.new("b.txt"), Path.new("e.txt"))?
    write.remove_file(Path.new("e.txt"))?
    write.remove_dir(Path.new("c"))?
    let file = fs.write_only().restrict_to_file("root/d.txt")
    file.write("d")?
    file.append("d")?
    let size = fs.read_only().restrict_to_file("root/d.txt").metadata()?
    let listener = net.listen_only().restrict_to_loopback(0)
    let host = net.restrict_to("127.0.0.1", listener.port())
    let client = host.connect()?
    let server = listener.accept()?
    client.send("ping")?
    let heard = server.receive_bytes()?
    server.send_bytes(heard)?
    io.println(client.receive()?)?
    io.println(server.host())?
    client.close()
    io.println(clock.now_millis().to_string())?
    io.println(random.next_int().to_string())?
    let line = input.read_line()?
    Ok(())
}
"#;
