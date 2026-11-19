//! The executable file for Windows: a PE32+ console executable for
//! x86_64 that the Windows loader maps, importing functions only from
//! the DLLs ADR 0020 allows: today `kernel32.dll` and
//! `bcryptprimitives.dll`, and `ntdll.dll` once a call needs it (ADR 0011,
//! ADR 0020). The format is Microsoft's, as
//! `docs/research/2026-11-windows-kernel32-calls.md` cites it
//! (`pe-format.md`).
//!
//! - An MS-DOS header whose only use is `e_lfanew`, the PE signature, the
//!   COFF file header and the PE32+ optional header, then four section
//!   headers.
//! - `.text` (read and execute): the code. `.rdata` (read-only): the
//!   read-only data, then the import directory, the import lookup
//!   tables, the hint/name table and the DLL names. `.data` (read and
//!   write): the import address table, which the loader fills, then the
//!   zero-initialised writable data, which takes no bytes in the file.
//!   `.reloc` (read-only, discardable): a base relocation table with no
//!   fixups. Code is never writable and data never executable.
//! - Position-independent: every address in the code is relative to
//!   `rip`, so no fixup is needed. The `.reloc` table is there only so
//!   that the loader may place the image anywhere: address-space layout
//!   randomization with high entropy (`DYNAMIC_BASE`,
//!   `HIGH_ENTROPY_VA`), and data execution prevention (`NX_COMPAT`).
//! - The import table names every function of the platform's import
//!   list, always all of them, bound at load: no lazy binding and no
//!   stubs.
//! - No timestamp and no checksum (both 0), so the file depends on the
//!   program and the target alone: every host writes the same bytes.
//! - The stack the loader gives the process's thread is reserved as the
//!   program needs (ADR 0020): `SizeOfStackReserve`.

use crate::target::{Code, Layout};

/// The image's preferred address: the default of Microsoft's linker for
/// a 64-bit executable, above 4 GiB. The loader moves it.
pub const IMAGE_BASE: u64 = 0x1_4000_0000;
/// The alignment of sections in memory: x86_64's page.
pub const SECTION_ALIGNMENT: u32 = 0x1000;
/// The alignment of sections in the file: the format's default.
pub const FILE_ALIGNMENT: u32 = 0x200;

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE_AMD64: u16 = 0x8664;
/// `IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_LARGE_ADDRESS_AWARE`: no
/// `IMAGE_FILE_RELOCS_STRIPPED`, so the image may move.
pub const CHARACTERISTICS: u16 = 0x0022;
/// The PE32+ optional header's magic.
pub const PE32_PLUS: u16 = 0x020b;
/// `IMAGE_SUBSYSTEM_WINDOWS_CUI`: a console program.
pub const SUBSYSTEM_CONSOLE: u16 = 3;
/// `HIGH_ENTROPY_VA | DYNAMIC_BASE | NX_COMPAT | TERMINAL_SERVER_AWARE`.
pub const DLL_CHARACTERISTICS: u16 = 0x0020 | 0x0040 | 0x0100 | 0x8000;
/// Windows Vista (6.0), the oldest release the optional header can name
/// with high-entropy address randomization in effect from Windows 8.
pub const OS_VERSION: (u16, u16) = (6, 0);

/// Section characteristics: `CNT_CODE | MEM_EXECUTE | MEM_READ`.
pub const SCN_TEXT: u32 = 0x6000_0020;
/// `CNT_INITIALIZED_DATA | MEM_READ`.
pub const SCN_RDATA: u32 = 0x4000_0040;
/// `CNT_INITIALIZED_DATA | MEM_READ | MEM_WRITE`.
pub const SCN_DATA: u32 = 0xc000_0040;
/// `CNT_INITIALIZED_DATA | MEM_DISCARDABLE | MEM_READ`.
pub const SCN_RELOC: u32 = 0x4200_0040;

/// The data directories the optional header has: all sixteen.
const DIRECTORIES: u32 = 16;
/// The directories this file fills, by index.
const DIRECTORY_IMPORT: usize = 1;
const DIRECTORY_BASERELOC: usize = 5;
const DIRECTORY_IAT: usize = 12;

/// Where the PE signature starts, which `e_lfanew` says: straight after
/// the 64-byte MS-DOS header, with no MS-DOS program.
const PE_AT: u32 = 64;
/// The sizes of the COFF header, the PE32+ optional header and a
/// section header.
const COFF_HEADER: u32 = 20;
const OPTIONAL_HEADER: u32 = 112 + 8 * DIRECTORIES;
const SECTION_HEADER: u32 = 40;
const SECTIONS: u32 = 4;

/// What the stack of the process's thread is: its reserve, and how much
/// of it is committed from the start.
pub const STACK_COMMIT: u64 = 0x1000;
/// What the process heap reserves and commits: the linker's defaults.
/// Oska never uses it, but the loader makes it.
const HEAP_RESERVE: u64 = 0x10_0000;
const HEAP_COMMIT: u64 = 0x1000;

/// A function the executable imports: its DLL and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import<'a> {
    pub library: &'a str,
    pub name: &'a str,
}

/// The slot of the import address table that holds the `index`th of
/// `imports`, in 8-byte words from its start: each DLL's slots in a row,
/// each run ended by a null slot, the DLLs in the order of their first
/// import.
pub fn slot(imports: &[Import], index: usize) -> usize {
    index + libraries(&imports[..=index]).len() - 1
}

/// The DLLs of `imports`, each once, in the order of their first import.
fn libraries<'a>(imports: &[Import<'a>]) -> Vec<&'a str> {
    let mut libraries: Vec<&str> = Vec::new();
    for import in imports {
        if !libraries.contains(&import.library) {
            libraries.push(import.library);
        }
    }
    libraries
}

/// A section's header.
#[derive(Debug, Clone, Copy)]
struct Section {
    name: &'static [u8; 8],
    rva: u32,
    virtual_size: u32,
    raw_at: u32,
    raw_size: u32,
    flags: u32,
}

/// The import directory, the lookup tables, the hint/name table and the
/// DLL names, laid out from `at`: the bytes, and the address table's
/// contents as the file holds them (the lookup table's).
struct ImportTables {
    bytes: Vec<u8>,
    address_table: Vec<u8>,
}

/// The executable holding `text`, `rodata`, `data_size` bytes of zeroed
/// writable data and an import table of `imports`, every DLL's imports
/// together, with a stack of `stack_reserve` bytes.
pub fn write(
    text: &dyn Code,
    rodata: &[u8],
    data_size: u32,
    imports: &[Import],
    stack_reserve: u64,
) -> Vec<u8> {
    let headers = PE_AT + 4 + COFF_HEADER + OPTIONAL_HEADER + SECTIONS * SECTION_HEADER;
    let headers_size = headers.next_multiple_of(FILE_ALIGNMENT);
    let rva_after = |section: &Section| {
        (section.rva + section.virtual_size.max(1)).next_multiple_of(SECTION_ALIGNMENT)
    };
    let raw_after = |section: &Section| section.raw_at + section.raw_size;
    let raw = |size: u32| size.next_multiple_of(FILE_ALIGNMENT);

    let text_size = u32::try_from(text.size()).unwrap_or(u32::MAX);
    let mut text_section = Section {
        name: b".text\0\0\0",
        rva: SECTION_ALIGNMENT,
        virtual_size: text_size,
        raw_at: headers_size,
        raw_size: raw(text_size),
        flags: SCN_TEXT,
    };
    // `.rdata`: the read-only data, then the import tables.
    let rdata_rva = rva_after(&text_section);
    let imports_at = (rodata.len() as u32).next_multiple_of(16);
    // `.data`: the import address table, then the writable data.
    let address_table_size = 8 * (imports.len() + libraries(imports).len()) as u32;
    let data_at = address_table_size.next_multiple_of(16);
    // The import tables name the address table's place, which follows
    // `.rdata`: lay them out once to learn their size, then again.
    let probe = import_tables(imports, rdata_rva + imports_at, 0);
    let rdata_size = imports_at + probe.bytes.len() as u32;
    let mut rdata = Section {
        name: b".rdata\0\0",
        rva: rdata_rva,
        virtual_size: rdata_size,
        raw_at: raw_after(&text_section),
        raw_size: raw(rdata_size),
        flags: SCN_RDATA,
    };
    let mut data = Section {
        name: b".data\0\0\0",
        rva: rva_after(&rdata),
        virtual_size: data_at + data_size,
        raw_at: raw_after(&rdata),
        raw_size: raw(address_table_size),
        flags: SCN_DATA,
    };
    let tables = import_tables(imports, rdata.rva + imports_at, data.rva);
    // One block for `.text`'s first page, with one padding entry: a
    // valid table with no fixup in it.
    let relocations: [u8; 12] = {
        let mut block = [0u8; 12];
        block[..4].copy_from_slice(&text_section.rva.to_le_bytes());
        block[4..8].copy_from_slice(&12u32.to_le_bytes());
        block
    };
    let reloc = Section {
        name: b".reloc\0\0",
        rva: rva_after(&data),
        virtual_size: relocations.len() as u32,
        raw_at: raw_after(&data),
        raw_size: raw(relocations.len() as u32),
        flags: SCN_RELOC,
    };
    let image_size = rva_after(&reloc);
    // A section with nothing in the file has no place in it.
    for section in [&mut text_section, &mut rdata, &mut data] {
        if section.raw_size == 0 {
            section.raw_at = 0;
        }
    }

    let layout = Layout {
        rodata: u64::from(rdata.rva),
        code: u64::from(text_section.rva),
        data: u64::from(data.rva + data_at),
        got: u64::from(data.rva),
    };
    let linked = text.link(&layout);

    let mut file = Vec::new();
    // The MS-DOS header: `MZ`, and `e_lfanew` at 0x3c.
    file.extend_from_slice(b"MZ");
    file.resize(0x3c, 0);
    put32(&mut file, PE_AT);
    file.extend_from_slice(b"PE\0\0");
    // The COFF file header.
    put16(&mut file, MACHINE_AMD64);
    put16(&mut file, SECTIONS as u16);
    put32(&mut file, 0); // TimeDateStamp
    put32(&mut file, 0); // PointerToSymbolTable
    put32(&mut file, 0); // NumberOfSymbols
    put16(&mut file, OPTIONAL_HEADER as u16);
    put16(&mut file, CHARACTERISTICS);
    // The optional header's standard fields.
    put16(&mut file, PE32_PLUS);
    file.extend_from_slice(&[0, 0]); // the linker's version
    put32(&mut file, text_section.raw_size); // SizeOfCode
    put32(&mut file, rdata.raw_size + data.raw_size + reloc.raw_size);
    put32(&mut file, 0); // SizeOfUninitializedData
    put32(&mut file, text_section.rva + text.entry() as u32);
    put32(&mut file, text_section.rva); // BaseOfCode
                                        // Its Windows-specific fields.
    put64(&mut file, IMAGE_BASE);
    put32(&mut file, SECTION_ALIGNMENT);
    put32(&mut file, FILE_ALIGNMENT);
    put16(&mut file, OS_VERSION.0);
    put16(&mut file, OS_VERSION.1);
    put32(&mut file, 0); // the image's version
    put16(&mut file, OS_VERSION.0);
    put16(&mut file, OS_VERSION.1);
    put32(&mut file, 0); // Win32VersionValue
    put32(&mut file, image_size);
    put32(&mut file, headers_size);
    put32(&mut file, 0); // CheckSum
    put16(&mut file, SUBSYSTEM_CONSOLE);
    put16(&mut file, DLL_CHARACTERISTICS);
    put64(&mut file, stack_reserve);
    put64(&mut file, STACK_COMMIT);
    put64(&mut file, HEAP_RESERVE);
    put64(&mut file, HEAP_COMMIT);
    put32(&mut file, 0); // LoaderFlags
    put32(&mut file, DIRECTORIES);
    let mut directories = [(0u32, 0u32); DIRECTORIES as usize];
    directories[DIRECTORY_IMPORT] = (
        rdata.rva + imports_at,
        20 * (libraries(imports).len() as u32 + 1),
    );
    directories[DIRECTORY_BASERELOC] = (reloc.rva, relocations.len() as u32);
    directories[DIRECTORY_IAT] = (data.rva, address_table_size);
    for (rva, size) in directories {
        put32(&mut file, rva);
        put32(&mut file, size);
    }
    for section in [&text_section, &rdata, &data, &reloc] {
        file.extend_from_slice(section.name);
        put32(&mut file, section.virtual_size);
        put32(&mut file, section.rva);
        put32(&mut file, section.raw_size);
        put32(&mut file, section.raw_at);
        put32(&mut file, 0); // PointerToRelocations
        put32(&mut file, 0); // PointerToLinenumbers
        put16(&mut file, 0);
        put16(&mut file, 0);
        put32(&mut file, section.flags);
    }

    let mut place = |at: u32, bytes: &[u8]| {
        let at = at as usize;
        if file.len() < at {
            file.resize(at, 0);
        }
        file.extend_from_slice(bytes);
    };
    place(text_section.raw_at, &linked);
    place(rdata.raw_at, rodata);
    place(rdata.raw_at + imports_at, &tables.bytes);
    place(data.raw_at, &tables.address_table);
    place(reloc.raw_at, &relocations);
    file.resize((reloc.raw_at + reloc.raw_size) as usize, 0);
    file
}

/// The import tables for `imports`, laid out from the address `at`, for
/// an address table at `address_table`: the import directory (one
/// descriptor per DLL and a null one), then each DLL's lookup table
/// (ended by a null entry), then the hint/name entries, then the DLL
/// names. The address table holds what the lookup tables hold until the
/// loader binds it.
fn import_tables(imports: &[Import], at: u32, address_table: u32) -> ImportTables {
    let libraries = libraries(imports);
    let directory = 20 * (libraries.len() as u32 + 1);
    let lookup_at = at + directory;
    let lookup_size = 8 * (imports.len() + libraries.len()) as u32;
    // The hint/name entries: a 2-byte hint (0: none), the name and a NUL,
    // each at an even address.
    let mut names = Vec::new();
    let names_at = lookup_at + lookup_size;
    let mut entries = Vec::new();
    for import in imports {
        entries.push(names_at + names.len() as u32);
        put16(&mut names, 0);
        names.extend_from_slice(import.name.as_bytes());
        names.push(0);
        if names.len() % 2 == 1 {
            names.push(0);
        }
    }
    let mut library_names = Vec::new();
    for library in &libraries {
        library_names.push(names_at + names.len() as u32);
        names.extend_from_slice(library.as_bytes());
        names.push(0);
    }

    let mut lookup = Vec::new();
    let mut bytes = Vec::new();
    for (k, library) in libraries.iter().enumerate() {
        let first = imports
            .iter()
            .position(|i| i.library == *library)
            .unwrap_or(0);
        let table = 8 * slot(imports, first) as u32;
        put32(&mut bytes, lookup_at + table); // OriginalFirstThunk
        put32(&mut bytes, 0); // TimeDateStamp
        put32(&mut bytes, 0); // ForwarderChain
        put32(&mut bytes, library_names[k]);
        put32(&mut bytes, address_table + table); // FirstThunk
        for (i, import) in imports.iter().enumerate() {
            if import.library == *library {
                put64(&mut lookup, u64::from(entries[i]));
            }
        }
        put64(&mut lookup, 0);
    }
    bytes.resize(directory as usize, 0);
    bytes.extend_from_slice(&lookup);
    bytes.extend_from_slice(&names);
    ImportTables {
        bytes,
        address_table: lookup,
    }
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Code of `size` bytes that says, at each 8-byte word, where the
    /// layout put the import table, so a test can check it.
    struct Fake {
        size: u64,
        entry: u64,
    }

    impl Code for Fake {
        fn size(&self) -> u64 {
            self.size
        }
        fn entry(&self) -> u64 {
            self.entry
        }
        fn link(&self, layout: &Layout) -> Vec<u8> {
            let mut code = layout.got.to_le_bytes().to_vec();
            code.extend_from_slice(&layout.data.to_le_bytes());
            code.extend_from_slice(&layout.rodata.to_le_bytes());
            code.resize(self.size as usize, 0xcc);
            code
        }
    }

    const IMPORTS: [Import; 4] = [
        Import {
            library: "KERNEL32.dll",
            name: "GetLastError",
        },
        Import {
            library: "KERNEL32.dll",
            name: "ExitProcess",
        },
        Import {
            library: "ntdll.dll",
            name: "NtClose",
        },
        Import {
            library: "bcryptprimitives.dll",
            name: "ProcessPrng",
        },
    ];

    #[test]
    fn each_dll_has_its_own_run_of_slots() {
        assert_eq!(
            (0..4).map(|i| slot(&IMPORTS, i)).collect::<Vec<_>>(),
            [0, 1, 3, 5]
        );
    }

    #[test]
    fn the_layout_places_data_after_the_address_table() {
        let file = write(
            &Fake {
                size: 100,
                entry: 8,
            },
            b"rodata",
            64,
            &IMPORTS,
            0x10_0000,
        );
        let at = 0x200;
        let word = |i: usize| {
            u64::from_le_bytes(
                file[at + 8 * i..at + 8 * i + 8]
                    .try_into()
                    .expect("8 bytes"),
            )
        };
        // `.text` at 0x1000, `.rdata` at 0x2000, `.data` at 0x3000.
        assert_eq!(word(0), 0x3000);
        // 7 slots (4 imports and 3 nulls): 56 bytes, then data at 64.
        assert_eq!(word(1), 0x3040);
        assert_eq!(word(2), 0x2000);
        assert_eq!(file.len() % FILE_ALIGNMENT as usize, 0);
    }
}
