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
use crate::ir::Module;
use crate::{aarch64, codes, elf, x86_64, NOWHERE};

/// A target platform the backend compiles for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// Linux on aarch64: a static ELF that makes system calls directly.
    LinuxAarch64,
    /// Linux on x86_64: the same, with x86_64's instructions, system-call
    /// numbers and `std.rt` table.
    LinuxX86_64,
}

/// Every supported platform, by name, in the order messages list them:
/// its target, or the release that adds its backend.
const PLATFORMS: [(&str, Result<Target, &str>); 5] = [
    ("linux-aarch64", Ok(Target::LinuxAarch64)),
    ("linux-x86_64", Ok(Target::LinuxX86_64)),
    ("macos-aarch64", Err("v0.5.0")),
    ("macos-x86_64", Err("v0.5.0")),
    ("windows-x86_64", Err("v0.6.0")),
];

impl Target {
    /// Every target the backend compiles for, in the order `oskac build
    /// --target all` builds them.
    pub const ALL: [Target; 2] = [Target::LinuxAarch64, Target::LinuxX86_64];

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

    /// The platform's name, as release assets name `oskac` builds.
    pub fn name(self) -> &'static str {
        match self {
            Target::LinuxAarch64 => "linux-aarch64",
            Target::LinuxX86_64 => "linux-x86_64",
        }
    }

    /// What the backend gives `std.rt` for this target: its system-call
    /// numbers and its table.
    pub(crate) fn platform(self) -> Platform {
        match self {
            Target::LinuxAarch64 => intrinsics::LINUX_AARCH64,
            Target::LinuxX86_64 => intrinsics::LINUX_X86_64,
        }
    }

    /// How many arguments a call passes in registers: the rest go through
    /// the module's extra arguments' area.
    pub(crate) fn register_args(self) -> usize {
        match self {
            Target::LinuxAarch64 => aarch64::REGISTER_ARGS,
            Target::LinuxX86_64 => x86_64::REGISTER_ARGS,
        }
    }

    /// Select and encode `module`'s instructions for this target. Fails
    /// only when the code is too large for the target's branches.
    pub(crate) fn emit(self, module: &Module) -> Result<Box<dyn Code>, String> {
        match self {
            Target::LinuxAarch64 => Ok(Box::new(aarch64::emit(module)?)),
            Target::LinuxX86_64 => Ok(Box::new(x86_64::emit(module)?)),
        }
    }

    /// What the executable's ELF header and segments say for this target.
    pub(crate) fn elf(self) -> elf::Machine {
        match self {
            // 64 KiB: the largest page an aarch64 Linux kernel uses.
            Target::LinuxAarch64 => elf::Machine {
                e_machine: elf::EM_AARCH64,
                page: 0x1_0000,
            },
            // 4 KiB: x86_64 Linux's only base page.
            Target::LinuxX86_64 => elf::Machine {
                e_machine: elf::EM_X86_64,
                page: 0x1000,
            },
        }
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
