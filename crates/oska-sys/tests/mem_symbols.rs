// SPDX-License-Identifier: Apache-2.0
//! The musl build for x86_64 takes `memcpy` and `memmove` from `oska-sys`
//! (spec #577, ticket #581), and through them every caller in the process,
//! musl's own `realloc` included.
//!
//! The test reads this test executable's symbol table. musl's x86_64
//! `memcpy.s` defines `memcpy` and the label `__memcpy_fwd` in one object
//! file, and its `memmove.s` jumps to that label. If the linker took musl's
//! `memcpy`, the label would be in the symbol table, and the two `memcpy`
//! definitions would clash. So `memcpy` and `memmove` are defined, and
//! `__memcpy_fwd` is not, only when both are the `oska-sys` functions.
//! Both functions are also called through the C symbols, at sizes of each
//! branch, to show the symbols work.

#![cfg(all(target_os = "linux", target_env = "musl", target_arch = "x86_64"))]

use std::fs;

// The declarations use `u8` pointers, as the definitions do.
#[allow(suspicious_runtime_symbol_definitions)]
extern "C" {
    fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8;
}

fn word(bytes: &[u8], at: usize, width: usize) -> usize {
    let mut v = 0usize;
    for i in (0..width).rev() {
        v = (v << 8) | bytes[at + i] as usize;
    }
    v
}

/// The names of the symbols this executable defines (ELF64, little-endian).
fn defined_symbols() -> Vec<String> {
    let exe = fs::read("/proc/self/exe").expect("read the test executable");
    assert_eq!(&exe[..4], b"\x7fELF", "not an ELF file");
    assert_eq!(exe[4], 2, "not ELF64");
    let shoff = word(&exe, 0x28, 8);
    let shentsize = word(&exe, 0x3a, 2);
    let shnum = word(&exe, 0x3c, 2);
    let section = |i: usize| shoff + i * shentsize;
    let mut names = Vec::new();
    for i in 0..shnum {
        let at = section(i);
        if word(&exe, at + 4, 4) != 2 {
            continue; // not SHT_SYMTAB
        }
        let offset = word(&exe, at + 0x18, 8);
        let size = word(&exe, at + 0x20, 8);
        let strtab = section(word(&exe, at + 0x28, 4));
        let str_offset = word(&exe, strtab + 0x18, 8);
        for entry in 0..size / 24 {
            let sym = offset + entry * 24;
            let name_at = str_offset + word(&exe, sym, 4);
            let shndx = word(&exe, sym + 6, 2);
            if shndx == 0 {
                continue; // undefined
            }
            let end = exe[name_at..].iter().position(|&b| b == 0).unwrap();
            names.push(String::from_utf8_lossy(&exe[name_at..name_at + end]).into_owned());
        }
    }
    names
}

#[test]
fn the_executable_has_oskas_memcpy_and_memmove_not_musls() {
    // Use the crate, so that the linker takes it (it would drop an unused one).
    oska_sys::copy_slice(&mut [0u8; 1], &[1u8], oska_sys::MemPath::Detect);
    let names = defined_symbols();
    assert!(
        !names.is_empty(),
        "the test executable has no symbol table (stripped?)"
    );
    for wanted in ["memcpy", "memmove"] {
        assert_eq!(
            names.iter().filter(|n| *n == wanted).count(),
            1,
            "expected exactly one definition of {wanted}"
        );
    }
    assert!(
        !names.iter().any(|n| n == "__memcpy_fwd"),
        "musl's x86_64 memcpy.o is linked: `__memcpy_fwd` is defined"
    );
}

#[test]
fn the_c_symbols_copy_and_move() {
    for n in [0usize, 1, 3, 9, 17, 40, 70, 3000, 70_000] {
        let src: Vec<u8> = (0..n).map(|i| (i * 7 + 1) as u8).collect();
        let mut dst = vec![0u8; n];
        // SAFETY: two separate buffers of `n` bytes each.
        let got = unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), n) };
        assert_eq!(got, dst.as_mut_ptr());
        assert_eq!(dst, src);

        let mut buf = vec![0u8; n + 5];
        buf[..n].copy_from_slice(&src);
        let base = buf.as_mut_ptr();
        // SAFETY: `n + 5` bytes; both ranges of `n` bytes lie inside, and
        // `memmove` accepts overlap.
        unsafe { memmove(base.add(5), base, n) };
        assert_eq!(&buf[5..], &src[..]);
    }
}
