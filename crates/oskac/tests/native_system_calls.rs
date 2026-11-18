//! The system calls a Linux executable can make (#366): the reshape of
//! the backend's `Platform` into `Platform.call`, and the `std.rt`
//! contracts it brought, change the executables' bytes but not which
//! system calls they make. Each target's executable is built on every
//! host with `oskac build --target`, and the numbers are read back from
//! its code: on aarch64 every `svc #0` and the `movz x8, #n` just before
//! it, on x86_64 every `syscall` and the `mov eax, n` (or, for 0, `xor
//! eax, eax`) just before it. The numbers expected are the kernel's own
//! tables', not the backend's.

mod common;

use common::portable::Scratch;
use std::collections::BTreeSet;
use std::process::Command;

/// A program that reaches every system call `std.rt` makes: the standard
/// streams, the clock and randomness, and confined reads, writes,
/// listings and directories, whose paths are resolved against the
/// working directory.
const EVERY_CALL: &str = r#"fn main(io: Stdout, input: Stdin, fs: Fs, clock: Clock, random: Random) -> Result<(), IoError> {
    let read = fs.read_only().restrict_to("root")
    let write = fs.write_only().restrict_to("root")
    let names = read.list_dir(Path.new("sub"))?
    let text = read.read_to_string(Path.new("a.txt"))?
    write.write(Path.new("b.txt"), "b")?
    write.create_dir(Path.new("c"))?
    let file = fs.write_only().restrict_to_file("root/d.txt")
    file.write("d")?
    io.println(clock.now_millis().to_string())?
    io.println(random.next_int().to_string())?
    let line = input.read_line()?
    Ok(())
}
"#;

/// `EVERY_CALL`'s executable for `target`.
fn build(target: &str) -> Vec<u8> {
    let dir = Scratch::new("native-system-calls", target);
    std::fs::write(dir.join("every.osk"), EVERY_CALL).expect("should write the program");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "every.osk", "--target", target, "-o", "every"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(dir.join("every")).expect("the executable should exist")
}

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> usize {
    let word = u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    usize::try_from(word).expect("an offset in the file")
}

/// The bytes of the ELF file's executable segment.
fn code(elf: &[u8]) -> &[u8] {
    assert_eq!(&elf[..4], b"\x7fELF");
    let (phoff, phentsize, phnum) = (u64_at(elf, 0x20), u16_at(elf, 0x36), u16_at(elf, 0x38));
    (0..phnum)
        .map(|i| phoff + i * phentsize)
        // PT_LOAD, with PF_X.
        .find(|&ph| u32_at(elf, ph) == 1 && u32_at(elf, ph + 4) & 1 != 0)
        .map(|ph| {
            let offset = u64_at(elf, ph + 8);
            &elf[offset..offset + u64_at(elf, ph + 32)]
        })
        .expect("an executable segment")
}

/// The numbers of the system calls in aarch64 code.
fn aarch64_calls(code: &[u8]) -> BTreeSet<u32> {
    let words: Vec<u32> = code.chunks_exact(4).map(|w| u32_at(w, 0)).collect();
    words
        .windows(2)
        .filter(|pair| pair[1] == 0xd400_0001)
        .map(|pair| {
            // `movz x8, #n`.
            assert_eq!(pair[0] & 0xffe0_001f, 0xd280_0008, "{:#x}", pair[0]);
            (pair[0] >> 5) & 0xffff
        })
        .collect()
}

/// The numbers of the system calls in x86_64 code.
fn x86_64_calls(code: &[u8]) -> BTreeSet<u32> {
    (7..code.len())
        .filter(|&at| code[at..].starts_with(&[0x0f, 0x05]))
        .filter_map(|at| {
            if code[at - 5] == 0xb8 {
                Some(u32_at(code, at - 4))
            } else if code[at - 2..at] == [0x33, 0xc0] {
                Some(0)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_linux_aarch64_executable_makes_the_system_calls_it_made_before() {
    // `asm-generic/unistd.h`: getcwd, fcntl, ioctl, mkdirat, ftruncate,
    // openat, close, getdents64, read, write, readlinkat, fstat,
    // exit_group, clock_gettime, kill, sigaltstack, rt_sigaction, getpid,
    // munmap, mmap, getrandom.
    let expected = [
        17, 25, 29, 34, 46, 56, 57, 61, 63, 64, 78, 80, 94, 113, 129, 132, 134, 172, 215, 222, 278,
    ];
    let elf = build("linux-aarch64");
    assert_eq!(aarch64_calls(code(&elf)), BTreeSet::from(expected));
}

#[test]
fn a_linux_x86_64_executable_makes_the_system_calls_it_made_before() {
    // `arch/x86/entry/syscalls/syscall_64.tbl`: read, write, close, fstat,
    // mmap, munmap, rt_sigaction, ioctl, getpid, kill, fcntl, ftruncate,
    // getcwd, sigaltstack, getdents64, clock_gettime, exit_group, openat,
    // mkdirat, readlinkat, getrandom.
    let expected = [
        0, 1, 3, 5, 9, 11, 13, 16, 39, 62, 72, 77, 79, 131, 217, 228, 231, 257, 258, 267, 318,
    ];
    let elf = build("linux-x86_64");
    assert_eq!(x86_64_calls(code(&elf)), BTreeSet::from(expected));
}
