//! Target platforms: what an executable is built for, and the code
//! generator and file each one gets.
//!
//! A target platform is named as release assets name `oskac` builds,
//! such as `linux-aarch64` (GLOSSARY: Supported platform, Target
//! platform). The backend takes the target as an input and never reads
//! the host, so every host writes the same bytes for the same program
//! and target.
//!
//! The backend's code (`ir.rs`) is the same for every target. A target
//! adds a code generator, which turns a module into [`Code`], and a file
//! format, which lays the code out and asks it to [`Code::link`] itself
//! there. To add a target: a variant of [`Target`], its row in
//! [`PLATFORMS`], its code generator, its [`Platform`] (system-call
//! numbers, and its table in `std.rt`), and its arms in the methods of
//! [`Target`]; no other target's code changes.

use oska_syntax::Diagnostic;

use crate::intrinsics::{self, Platform};
use crate::ir::{Module, Symbol};
use crate::{aarch64, codes, elf, macho, x86_64, NOWHERE};

/// A target platform the backend compiles for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// Linux on aarch64: a static ELF that makes system calls directly.
    LinuxAarch64,
    /// Linux on x86_64: the same, with x86_64's instructions, system-call
    /// numbers and `std.rt` table.
    LinuxX86_64,
    /// macOS on aarch64 (Apple silicon): a Mach-O that `dyld` loads,
    /// calling the system through `libSystem`, ad-hoc signed.
    MacosAarch64,
    /// macOS on x86_64 (Intel): the same, with x86_64's instructions.
    MacosX86_64,
}

/// Every supported platform, by name, in the order messages list them:
/// its target, or the release that adds its backend.
const PLATFORMS: [(&str, Result<Target, &str>); 5] = [
    ("linux-aarch64", Ok(Target::LinuxAarch64)),
    ("linux-x86_64", Ok(Target::LinuxX86_64)),
    ("macos-aarch64", Ok(Target::MacosAarch64)),
    ("macos-x86_64", Ok(Target::MacosX86_64)),
    ("windows-x86_64", Err("v0.6.0")),
];

impl Target {
    /// Every target the backend compiles for, in the order `oskac build
    /// --target all` builds them.
    pub const ALL: [Target; 4] = [
        Target::LinuxAarch64,
        Target::LinuxX86_64,
        Target::MacosAarch64,
        Target::MacosX86_64,
    ];

    /// The targets `oskac run` and `oskac test` compile for when they are
    /// the host's: every target.
    const HOSTS: [Target; 4] = Target::ALL;

    /// The target for the supported platform named `platform`. An
    /// unknown name is refused with `E0393`, listing the names; a
    /// supported platform with no backend yet with `E0394`, naming the
    /// release that adds it.
    pub fn named(platform: &str) -> Result<Target, Diagnostic> {
        match PLATFORMS.iter().find(|(name, _)| *name == platform) {
            Some((_, Ok(target))) => Ok(*target),
            Some((_, Err(release))) => Err(Diagnostic::new(
                codes::NO_BACKEND,
                format!(
                    "`oskac build` cannot build for `{platform}` yet: its native backend \
                     comes in {release}; it builds for {}",
                    listed(Target::ALL.iter().map(|t| t.name()))
                ),
                NOWHERE,
            )),
            None => Err(Diagnostic::new(
                codes::UNKNOWN_PLATFORM,
                format!(
                    "`{platform}` is not a target platform: `--target` takes {}, or `all`",
                    listed(PLATFORMS.iter().map(|(name, _)| *name))
                ),
                NOWHERE,
            )),
        }
    }

    /// The target for the host `oskac` itself runs on, when its platform
    /// has a backend: what `oskac run` and `oskac test` compile for, and
    /// `oskac build` without `--target`. Only the CLI asks; nothing that
    /// compiles reads the host. A platform's name is the host's operating
    /// system and architecture as Rust names them, so no target needs a
    /// line here.
    pub fn host() -> Option<Target> {
        let host = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        Target::HOSTS
            .into_iter()
            .find(|target| target.name() == host)
    }

    /// The platform's name, as release assets name `oskac` builds.
    pub fn name(self) -> &'static str {
        match self {
            Target::LinuxAarch64 => "linux-aarch64",
            Target::LinuxX86_64 => "linux-x86_64",
            Target::MacosAarch64 => "macos-aarch64",
            Target::MacosX86_64 => "macos-x86_64",
        }
    }

    /// What the backend gives `std.rt` for this target: its system-call
    /// numbers and its table.
    pub(crate) fn platform(self) -> Platform {
        match self {
            Target::LinuxAarch64 => intrinsics::LINUX_AARCH64,
            Target::LinuxX86_64 => intrinsics::LINUX_X86_64,
            Target::MacosAarch64 => intrinsics::MACOS_AARCH64,
            Target::MacosX86_64 => intrinsics::MACOS_X86_64,
        }
    }

    /// How many arguments a call passes in registers: the rest go through
    /// the module's extra arguments' area.
    pub(crate) fn register_args(self) -> usize {
        match self {
            Target::LinuxAarch64 | Target::MacosAarch64 => aarch64::REGISTER_ARGS,
            Target::LinuxX86_64 | Target::MacosX86_64 => x86_64::REGISTER_ARGS,
        }
    }

    /// Select and encode `module`'s instructions for this target. Fails
    /// only when the code is too large for the target's branches.
    pub(crate) fn emit(self, module: &Module) -> Result<Box<dyn Code>, String> {
        match self {
            Target::LinuxAarch64 | Target::MacosAarch64 => Ok(Box::new(aarch64::emit(module)?)),
            Target::LinuxX86_64 | Target::MacosX86_64 => Ok(Box::new(x86_64::emit(module)?)),
        }
    }

    /// The executable file for this target, holding `text`, `rodata`
    /// and `data_size` bytes of zeroed writable data.
    pub(crate) fn file(self, text: &dyn Code, rodata: &[u8], data_size: u32) -> Vec<u8> {
        let elf = |e_machine, page| {
            elf::write(&elf::Machine { e_machine, page }, text, rodata, data_size)
        };
        match self {
            // 64 KiB: the largest page an aarch64 Linux kernel uses.
            Target::LinuxAarch64 => elf(elf::EM_AARCH64, 0x1_0000),
            // 4 KiB: x86_64 Linux's only base page.
            Target::LinuxX86_64 => elf(elf::EM_X86_64, 0x1000),
            Target::MacosAarch64 => self.macho(
                &macho::Machine {
                    cputype: macho::CPU_TYPE_ARM64,
                    cpusubtype: macho::CPU_SUBTYPE_ARM64_ALL,
                    // 16 KiB: Apple silicon's page.
                    page: 0x4000,
                    // macOS 12, the first whose `dyld` binds chained
                    // fixups in an arm64 executable.
                    minos: 0x000c_0000,
                },
                text,
                rodata,
                data_size,
            ),
            Target::MacosX86_64 => self.macho(
                &macho::Machine {
                    cputype: macho::CPU_TYPE_X86_64,
                    cpusubtype: macho::CPU_SUBTYPE_X86_64_ALL,
                    // 4 KiB: an Intel Mac's page.
                    page: 0x1000,
                    // macOS 12, as on Apple silicon, so both macOS
                    // targets run on the same releases.
                    minos: 0x000c_0000,
                },
                text,
                rodata,
                data_size,
            ),
        }
    }
}

impl Target {
    /// A Mach-O for `machine`, importing this target's functions.
    fn macho(
        self,
        machine: &macho::Machine,
        text: &dyn Code,
        rodata: &[u8],
        data_size: u32,
    ) -> Vec<u8> {
        let imports: Vec<&str> = self.platform().imports.iter().map(|i| i.symbol).collect();
        macho::write(machine, text, rodata, data_size, &imports)
    }
}

/// `names` in a sentence: `` `a`, `b` and `c` ``.
fn listed<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<String> = names.map(|n| format!("`{n}`")).collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// Where the file put each part of the executable, as addresses from
/// the start of the loaded file.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub rodata: u64,
    pub code: u64,
    pub data: u64,
    /// The import table: one 8-byte slot per library function, which the
    /// dynamic loader fills ([`Symbol::Import`]). Nothing on Linux.
    pub got: u64,
}

impl Layout {
    /// Where `symbol` is, as an address from the start of the loaded
    /// file. A [`Symbol::Function`] holds the offset of the function's
    /// first byte in the code, as every code generator's [`Code`] holds
    /// it, not the function's id.
    pub fn address(&self, symbol: Symbol) -> u64 {
        match symbol {
            Symbol::Rodata(at) => self.rodata + u64::from(at),
            Symbol::Data(at) => self.data + u64::from(at),
            Symbol::Function(start) => self.code + start as u64,
            Symbol::Import(slot) => self.got + 8 * u64::from(slot),
        }
    }
}

/// A module's machine code for one target, before the file's layout
/// fixes the addresses it refers to: the interface between every code
/// generator and every file format.
pub trait Code {
    /// Its size in bytes.
    fn size(&self) -> u64;
    /// Where the process starts, in bytes from the start of the code.
    fn entry(&self) -> u64;
    /// Its bytes, placed as `layout` says, with every reference to data
    /// and to code resolved.
    fn link(&self, layout: &Layout) -> Vec<u8>;
}
