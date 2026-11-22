//! `oskac build --target windows-x86_64` (spec #155, ticket #176): a
//! PE32+ console executable that the Windows loader loads, importing only
//! from `kernel32.dll`, `ntdll.dll`, `bcryptprimitives.dll` and `ws2_32.dll` (ADR 0020).
//!
//! Every host checks the file against Microsoft's format (`pe-format.md`),
//! byte by byte, so the writer is developed without Windows. On Windows the
//! tests also run what they build and compare it with the interpreter.
//!
//! The case with golden output compares the exit code, stdout, stderr and
//! the files the build left with `tests/golden/build_windows/<case>.txt`;
//! regenerate it after an intended change with
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test build_windows` and review
//! the diff.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

const PLATFORM: &str = "windows-x86_64";

/// The functions every Windows executable imports, by DLL, in the order
/// of the import table (ADR 0020: the list is reviewed surface).
const IMPORTS: [(&str, &[&str]); 4] = [
    (
        "KERNEL32.dll",
        &[
            "GetLastError",
            "GetStdHandle",
            "WriteFile",
            "ReadFile",
            "GetConsoleMode",
            "WriteConsoleW",
            "ReadConsoleW",
            "VirtualAlloc",
            "VirtualFree",
            "ExitProcess",
            "GetCommandLineW",
            "GetEnvironmentStringsW",
            "AddVectoredExceptionHandler",
            "SetThreadStackGuarantee",
            "GetSystemTimePreciseAsFileTime",
            "CreateFileW",
            "CloseHandle",
            "GetFinalPathNameByHandleW",
            "SetEndOfFile",
        ],
    ),
    (
        "ntdll.dll",
        &[
            "NtCreateFile",
            "NtQueryInformationFile",
            "NtQueryDirectoryFile",
            "RtlNtStatusToDosError",
        ],
    ),
    ("bcryptprimitives.dll", &["ProcessPrng"]),
    (
        "WS2_32.dll",
        &[
            "WSAStartup",
            "socket",
            "connect",
            "bind",
            "listen",
            "accept",
            "getsockname",
            "getsockopt",
            "setsockopt",
            "send",
            "recv",
            "WSAPoll",
            "closesocket",
            "ioctlsocket",
        ],
    ),
];

/// A new, empty directory for `case`, holding `files`.
fn workspace(case: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build_windows")
        .join(case);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir
}

/// Run `oskac args` in `dir`.
fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// `src` built for Windows in the case directory `case`: the file.
fn build(case: &str, src: &str) -> Vec<u8> {
    let dir = workspace(case, &[("main.osk", src)]);
    let built = oskac(
        &dir,
        &["build", "main.osk", "--target", PLATFORM, "-o", "main.exe"],
    );
    assert!(built.status.success(), "{built:?}");
    std::fs::read(dir.join("main.exe")).expect("the executable should exist")
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("2 bytes"))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

/// The NUL-terminated text at `at`.
fn text_at(bytes: &[u8], at: usize) -> String {
    let end = bytes[at..]
        .iter()
        .position(|&b| b == 0)
        .map_or(bytes.len(), |n| at + n);
    String::from_utf8_lossy(&bytes[at..end]).into_owned()
}

/// What the headers say, by offset in the file.
struct Headers {
    /// The COFF file header, after the `PE\0\0` signature.
    coff: usize,
    /// The optional header.
    optional: usize,
    /// The section table.
    sections: Vec<Section>,
}

#[derive(Debug, Clone)]
struct Section {
    name: String,
    virtual_size: u32,
    rva: u32,
    raw_size: u32,
    raw_at: u32,
    flags: u32,
}

const SCN_CNT_CODE: u32 = 0x20;
const SCN_CNT_INITIALIZED_DATA: u32 = 0x40;
const SCN_MEM_DISCARDABLE: u32 = 0x0200_0000;
const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const SCN_MEM_READ: u32 = 0x4000_0000;
const SCN_MEM_WRITE: u32 = 0x8000_0000;

fn headers(file: &[u8]) -> Headers {
    assert_eq!(&file[..2], b"MZ", "the MS-DOS header");
    let pe = u32_at(file, 0x3c) as usize;
    assert_eq!(&file[pe..pe + 4], b"PE\0\0", "the PE signature");
    let coff = pe + 4;
    let count = u16_at(file, coff + 2) as usize;
    let optional = coff + 20;
    let table = optional + u16_at(file, coff + 16) as usize;
    let sections = (0..count)
        .map(|i| {
            let at = table + 40 * i;
            Section {
                name: text_at(&file[at..at + 8], 0),
                virtual_size: u32_at(file, at + 8),
                rva: u32_at(file, at + 12),
                raw_size: u32_at(file, at + 16),
                raw_at: u32_at(file, at + 20),
                flags: u32_at(file, at + 36),
            }
        })
        .collect();
    Headers {
        coff,
        optional,
        sections,
    }
}

impl Headers {
    /// The `index`th data directory: its address and size.
    fn directory(&self, file: &[u8], index: usize) -> (u32, u32) {
        let at = self.optional + 112 + 8 * index;
        (u32_at(file, at), u32_at(file, at + 8 - 4))
    }

    /// The file offset of the address `rva`, in the section that holds it
    /// in the file.
    fn offset(&self, rva: u32) -> usize {
        let section = self
            .sections
            .iter()
            .find(|s| rva >= s.rva && rva < s.rva + s.raw_size.max(s.virtual_size))
            .unwrap_or_else(|| panic!("no section holds {rva:#x}"));
        assert!(
            rva < section.rva + section.raw_size,
            "{rva:#x} is in the file"
        );
        (section.raw_at + rva - section.rva) as usize
    }
}

#[test]
fn it_names_a_windows_executable_ending_in_exe() {
    let case = "windows_x86_64";
    let dir = workspace(case, &[("hello.osk", HELLO)]);
    let args = ["build", "hello.osk", "--target", PLATFORM];
    let mut before = BTreeSet::new();
    files_in(&dir, &mut before);
    let output = oskac(&dir, &args);
    let mut after = BTreeSet::new();
    files_in(&dir, &mut after);
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- written\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        after
            .difference(&before)
            .map(|w| format!("{w}\n"))
            .collect::<String>(),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/build_windows")
        .join(format!("{case}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
    let file = std::fs::read(dir.join("hello.exe")).expect("the executable should exist");
    assert_eq!(&file[..2], b"MZ");
}

/// Every file name in `dir`.
fn files_in(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("the directory").flatten() {
        out.insert(entry.file_name().to_string_lossy().into_owned());
    }
}

/// The COFF header and the optional header say a PE32+ console
/// executable for x86_64 that may be loaded anywhere, with high-entropy
/// address randomization and data execution prevention, and no timestamp
/// or checksum.
#[test]
fn the_headers_say_a_relocatable_console_executable_for_x86_64() {
    let file = build("headers", HELLO);
    let h = headers(&file);
    // IMAGE_FILE_MACHINE_AMD64, four sections, no timestamp, no symbols.
    assert_eq!(u16_at(&file, h.coff), 0x8664);
    assert_eq!(u16_at(&file, h.coff + 2), 4);
    assert_eq!(u32_at(&file, h.coff + 4), 0, "TimeDateStamp");
    assert_eq!(u32_at(&file, h.coff + 8), 0);
    assert_eq!(u32_at(&file, h.coff + 12), 0);
    // A PE32+ optional header with all 16 data directories.
    assert_eq!(u16_at(&file, h.coff + 16), 240);
    // EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE, and not RELOCS_STRIPPED.
    let characteristics = u16_at(&file, h.coff + 18);
    assert_eq!(characteristics, 0x0022);
    assert_eq!(characteristics & 0x0001, 0, "the image may move");
    let o = h.optional;
    assert_eq!(u16_at(&file, o), 0x020b, "PE32+");
    let entry = u32_at(&file, o + 16);
    let text = &h.sections[0];
    assert!(
        entry >= text.rva && entry < text.rva + text.virtual_size,
        "the entry point is in .text"
    );
    assert_eq!(u32_at(&file, o + 20), text.rva, "BaseOfCode");
    let image_base = u64_at(&file, o + 24);
    assert!(image_base > u64::from(u32::MAX), "above 4 GiB");
    assert_eq!(image_base % 0x1_0000, 0, "on a 64 KiB boundary");
    let section_alignment = u32_at(&file, o + 32);
    let file_alignment = u32_at(&file, o + 36);
    assert_eq!(section_alignment, 0x1000);
    assert!(file_alignment.is_power_of_two() && (512..=0x1_0000).contains(&file_alignment));
    assert!(section_alignment >= file_alignment);
    assert_eq!(u16_at(&file, o + 40), 6, "MajorOperatingSystemVersion");
    assert_eq!(u16_at(&file, o + 48), 6, "MajorSubsystemVersion");
    assert_eq!(u32_at(&file, o + 52), 0, "Win32VersionValue");
    let image_size = u32_at(&file, o + 56);
    assert_eq!(image_size % section_alignment, 0, "SizeOfImage");
    let last = h.sections.last().expect("a section");
    assert_eq!(
        image_size,
        (last.rva + last.virtual_size).next_multiple_of(section_alignment)
    );
    let headers_size = u32_at(&file, o + 60);
    assert_eq!(headers_size % file_alignment, 0, "SizeOfHeaders");
    assert!(headers_size as usize >= h.optional + 240 + 40 * h.sections.len());
    assert_eq!(u32_at(&file, o + 64), 0, "CheckSum");
    assert_eq!(u16_at(&file, o + 68), 3, "IMAGE_SUBSYSTEM_WINDOWS_CUI");
    // HIGH_ENTROPY_VA, DYNAMIC_BASE, NX_COMPAT, TERMINAL_SERVER_AWARE.
    assert_eq!(u16_at(&file, o + 70), 0x8160);
    let stack_reserve = u64_at(&file, o + 72);
    let stack_commit = u64_at(&file, o + 80);
    assert_eq!(stack_reserve % 0x1_0000, 0, "in 64 KiB");
    assert!(
        stack_reserve >= 8 << 20,
        "the program's stack: {stack_reserve}"
    );
    assert!(stack_commit <= stack_reserve);
    assert_eq!(u32_at(&file, o + 108), 16, "NumberOfRvaAndSizes");
}

/// `.text` is the only code and never writable; nothing else executes.
/// Sections follow each other in memory and in the file, aligned.
#[test]
fn code_is_never_writable_and_data_never_executable() {
    let file = build("sections", HELLO);
    let h = headers(&file);
    let names: Vec<&str> = h.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, [".text", ".rdata", ".data", ".reloc"]);
    let flags: Vec<u32> = h.sections.iter().map(|s| s.flags).collect();
    assert_eq!(
        flags,
        [
            SCN_CNT_CODE | SCN_MEM_EXECUTE | SCN_MEM_READ,
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ,
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ | SCN_MEM_WRITE,
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_DISCARDABLE | SCN_MEM_READ,
        ]
    );
    for section in &h.sections {
        assert!(
            section.flags & (SCN_MEM_WRITE | SCN_MEM_EXECUTE) != (SCN_MEM_WRITE | SCN_MEM_EXECUTE),
            "{} is writable and executable",
            section.name
        );
    }
    let o = h.optional;
    let headers_size = u32_at(&file, o + 60);
    let mut rva = 0x1000;
    let mut raw = headers_size;
    for section in &h.sections {
        assert_eq!(section.rva, rva, "{} follows the one before", section.name);
        assert_eq!(section.raw_size % 0x200, 0, "{}", section.name);
        assert_eq!(section.raw_at, raw, "{}", section.name);
        assert!(section.virtual_size > 0, "{}", section.name);
        rva = (section.rva + section.virtual_size).next_multiple_of(0x1000);
        raw = section.raw_at + section.raw_size;
    }
    assert_eq!(file.len(), raw as usize, "nothing follows the last section");
    // The code fills `.text` from the start, and the writable data
    // beyond what the file holds is zero-filled by the loader.
    let data = &h.sections[2];
    assert!(data.virtual_size > data.raw_size);
}

/// The import table names only the platform's functions, from only
/// `kernel32.dll` and `bcryptprimitives.dll`, by name, in a fixed order;
/// each DLL's address table, as the file holds it, is its lookup table;
/// and the IAT directory covers them all.
#[test]
fn it_imports_only_the_reviewed_functions_of_kernel32_and_bcryptprimitives() {
    let file = build("imports", HELLO);
    let h = headers(&file);
    let (import_rva, import_size) = h.directory(&file, 1);
    assert_eq!(import_size, 20 * (IMPORTS.len() as u32 + 1));
    let (iat_rva, iat_size) = h.directory(&file, 12);
    let total: usize = IMPORTS.iter().map(|(_, names)| names.len() + 1).sum();
    assert_eq!(iat_size as usize, 8 * total);
    let data = &h.sections[2];
    assert_eq!(iat_rva, data.rva, "the address table starts `.data`");
    let mut next_iat = iat_rva;
    for (k, (library, names)) in IMPORTS.iter().enumerate() {
        let at = h.offset(import_rva) + 20 * k;
        let lookup = u32_at(&file, at);
        assert_eq!(u32_at(&file, at + 4), 0, "TimeDateStamp: not bound");
        assert_eq!(u32_at(&file, at + 8), 0, "ForwarderChain");
        assert_eq!(text_at(&file, h.offset(u32_at(&file, at + 12))), *library);
        let address = u32_at(&file, at + 16);
        assert_eq!(address, next_iat, "{library}'s address table");
        for (i, name) in names.iter().enumerate() {
            let entry = u64_at(&file, h.offset(lookup) + 8 * i);
            assert_eq!(entry >> 63, 0, "{name} is imported by name");
            assert_eq!(
                u64_at(&file, h.offset(address) + 8 * i),
                entry,
                "the file's address table holds the lookup table"
            );
            let hint = h.offset(entry as u32);
            assert_eq!(hint % 2, 0, "{name}'s hint/name entry is aligned");
            assert_eq!(text_at(&file, hint + 2), *name);
        }
        assert_eq!(u64_at(&file, h.offset(lookup) + 8 * names.len()), 0);
        assert_eq!(u64_at(&file, h.offset(address) + 8 * names.len()), 0);
        next_iat = address + 8 * (names.len() as u32 + 1);
    }
    let end = h.offset(import_rva) + 20 * IMPORTS.len();
    assert_eq!(&file[end..end + 20], &[0; 20], "the null descriptor");
    // The import tables are read-only data.
    let rdata = &h.sections[1];
    assert!(import_rva >= rdata.rva && import_rva + import_size <= rdata.rva + rdata.virtual_size);
}

/// The base relocation table is valid and makes no fixup: the code
/// addresses everything relative to `rip`, so the loader may place the
/// image anywhere.
#[test]
fn the_relocation_table_is_one_block_with_no_fixup() {
    let file = build("relocations", HELLO);
    let h = headers(&file);
    let (rva, size) = h.directory(&file, 5);
    let reloc = &h.sections[3];
    assert_eq!((rva, size), (reloc.rva, reloc.virtual_size));
    let at = h.offset(rva);
    assert_eq!(
        u32_at(&file, at),
        h.sections[0].rva,
        "the page of the block"
    );
    let block = u32_at(&file, at + 4);
    assert_eq!(block, size, "one block");
    assert_eq!(block % 4, 0);
    for entry in (8..block as usize).step_by(2) {
        assert_eq!(
            u16_at(&file, at + entry) >> 12,
            0,
            "IMAGE_REL_BASED_ABSOLUTE: padding, no fixup"
        );
    }
}

/// The same program gives the same bytes, and another program others.
#[test]
fn the_same_program_builds_to_the_same_bytes() {
    let a = build("same_a", HELLO);
    let b = build("same_b", HELLO);
    assert!(a == b, "two builds of one program differ");
    let c = build(
        "other",
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"bye\")\n}\n",
    );
    assert!(a != c);
}

const PROGRAMS: [(&str, &str); 5] = [
    ("hello", HELLO),
    (
        "integers",
        "fn fib(n: Int) -> Int {\n    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }\n}\n\n\
         fn main(io: Stdout) -> Result<(), IoError> {\n    var i = 0\n    while i < 20 {\n        \
         let f = fib(i)\n        io.println(\"fib({i}) = {f}\")?\n        i = i + 1\n    }\n    \
         io.println((-9223372036854775807 - 1).to_string())\n}\n",
    ),
    (
        "args",
        "fn main(io: Stdout, args: Args) -> Result<(), IoError> {\n    var i = 1\n    \
         while i < 6 {\n        match args.get(i) {\n            \
         Some(a) => io.println(\"argument {i}: [{a}]\")?\n            \
         None => io.println(\"no argument {i}\")?\n        }\n        i = i + 1\n    }\n    Ok(())\n}\n",
    ),
    (
        "error",
        "fn main(io: Stdout) -> Result<(), String> {\n    \
         let _ = io.println(\"before\")\n    Err(\"it failed\")\n}\n",
    ),
    (
        "panic",
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"before\")?\n    \
         let big = 9223372036854775807\n    io.println((big + 1).to_string())\n}\n",
    ),
];

/// The arguments the programs run with: quotes, backslashes, spaces and
/// text beyond ASCII, which Windows passes as one command line that the
/// program splits itself.
const ARGS: [&str; 4] = [
    "plain",
    "two words",
    "quote \" and \\ backslash\\",
    "héllo ✓",
];

/// Each program, built for Windows and run on Windows, does what the
/// interpreter does, byte for byte. Elsewhere it is only built.
#[test]
fn small_programs_run_on_windows_as_on_the_interpreter() {
    for (name, src) in PROGRAMS {
        let dir = workspace(&format!("run_{name}"), &[("main.osk", src)]);
        let mut run = vec!["run", "--interpret", "main.osk", "--"];
        run.extend(ARGS);
        let interpreted = oskac(&dir, &run);
        // 2 is a usage error: the program never ran.
        assert!(
            interpreted.status.code().is_some_and(|code| code != 2),
            "{name}: the interpreter: {interpreted:?}"
        );
        let built = oskac(
            &dir,
            &["build", "main.osk", "--target", PLATFORM, "-o", "main.exe"],
        );
        assert!(built.status.success(), "{name}: {built:?}");
        if !cfg!(all(windows, target_arch = "x86_64")) {
            continue;
        }
        let native = Command::new(dir.join("main.exe"))
            .args(ARGS)
            .current_dir(&dir)
            .output()
            .expect("the executable should start");
        assert_eq!(
            native.status.code(),
            interpreted.status.code(),
            "{name}: exit code; native stderr: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&native.stdout),
            String::from_utf8_lossy(&interpreted.stdout),
            "{name}: stdout"
        );
        assert_eq!(
            String::from_utf8_lossy(&native.stderr),
            String::from_utf8_lossy(&interpreted.stderr),
            "{name}: stderr"
        );
    }
}
