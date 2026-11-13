//! `oskac build`: check a source file or package and compile it with the
//! native backend (`oska-native`) to an executable for a target
//! platform, whatever the host.
//!
//! `--target <platform>` names the target platform as release assets
//! name `oskac` builds, such as `linux-aarch64`. Without it, the host's
//! platform is built for when it has a backend, else `linux-aarch64`.
//! An unknown name is refused with `E0393`, a supported platform with no
//! backend yet with `E0394`, both before anything is read, with exit
//! code 2, as a usage error.
//!
//! The output is the file's stem, or the package directory's name, in the
//! current directory, unless `-o` names it. `--target all` builds for
//! every target with a backend, into the directory `-o` names (the
//! current one without it), one file each named `<name>-<platform>`. Every
//! target is compiled before any file is written, so a refusal writes
//! nothing.
//!
//! Success prints nothing, or `{"output": path, "diagnostics": []}` with
//! `--format=json`; for `--target all`, `{"outputs": [path, ...],
//! "diagnostics": []}`. A refusal prints its diagnostics as `oskac check`
//! does, or as `{"output": null, "diagnostics": [...]}`. The exit code is
//! 1 for the checker's diagnostics and an unwritable output (`E0391`), and
//! 2 for a program the backend cannot compile yet (`E0371`, `E0390`), as
//! `oskac run`'s is for one the interpreter cannot run, and for a
//! `--target` it cannot build for.

use std::path::Path;
use std::process::ExitCode;

use oska_check::program::Program;
use oska_native::Target;
use oska_syntax::{json_escape, Diagnostic, Span};

use crate::{print_diagnostics, read_source, Flags, Places};

/// The host's supported platform, by name, if it is one.
const HOST: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some("linux-aarch64")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("linux-x86_64")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("macos-aarch64")
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some("macos-x86_64")
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("windows-x86_64")
} else {
    None
};

/// What `oskac build` builds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Targets {
    /// One target: one executable, at the output path.
    One(Target),
    /// `--target all`: every target with a backend, into the output
    /// directory.
    All,
}

impl Targets {
    /// The targets `--target`'s value names, or with none, the host's
    /// platform when it has a backend, else `linux-aarch64`.
    pub fn named(platform: Option<&str>) -> Result<Targets, Diagnostic> {
        match platform {
            Some("all") => Ok(Targets::All),
            Some(platform) => Target::named(platform).map(Targets::One),
            None => Ok(Targets::One(
                HOST.and_then(|host| Target::named(host).ok())
                    .unwrap_or(Target::LinuxAarch64),
            )),
        }
    }
}

/// How `oskac build` was asked to build: the output `-o` names, and the
/// targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options<'a> {
    pub output: Option<&'a str>,
    pub targets: Targets,
}

/// `oskac build` on the source file `path`.
pub fn file(path: &str, Flags { json, .. }: Flags, options: Options) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let report = oska_check::check_file(path, &src, oska_check::CheckOptions::default());
    let places = Places::File { path, src: &src };
    let Some(program) = report
        .program
        .as_ref()
        .filter(|_| report.diagnostics.is_empty())
    else {
        let diagnostics: Vec<Diagnostic> = report
            .diagnostics
            .into_iter()
            .map(|d| places.locate(d))
            .collect();
        return refuse(&diagnostics, json, ExitCode::FAILURE);
    };
    let stem = Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    compile(program, &places, &stem, options, json)
}

/// `oskac build` on the package in `dir`, which checked as `program`.
pub fn package(
    program: &Program,
    places: &Places,
    dir: &str,
    options: Options,
    json: bool,
) -> ExitCode {
    let name = std::fs::canonicalize(dir)
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "main".to_string());
    compile(program, places, &name, options, json)
}

/// Compile `program`, named `name`, for each target `options` asks for,
/// and write the executables.
fn compile(
    program: &Program,
    places: &Places,
    name: &str,
    options: Options,
    json: bool,
) -> ExitCode {
    let builds: Vec<(Target, String)> = match options.targets {
        Targets::One(target) => vec![(target, options.output.unwrap_or(name).to_string())],
        Targets::All => Target::ALL
            .iter()
            .map(|target| {
                let file = format!("{name}-{}", target.name());
                let path = match options.output {
                    Some(dir) => Path::new(dir).join(file).to_string_lossy().into_owned(),
                    None => file,
                };
                (*target, path)
            })
            .collect(),
    };
    let mut executables = Vec::with_capacity(builds.len());
    for (target, path) in builds {
        match oska_native::build(program, target, &|d| places.locate(d)) {
            Ok(bytes) => executables.push((path, bytes)),
            Err(refusal) => return refuse(&[places.locate(refusal)], json, ExitCode::from(2)),
        }
    }
    for (path, bytes) in &executables {
        if let Err(err) = write_executable(path, bytes) {
            return refuse(&[unwritable(path, &err)], json, ExitCode::FAILURE);
        }
    }
    if json {
        let paths: Vec<String> = executables
            .iter()
            .map(|(path, _)| format!("\"{}\"", json_escape(path)))
            .collect();
        match options.targets {
            Targets::One(_) => outln!("{{\"output\":{},\"diagnostics\":[]}}", paths.concat()),
            Targets::All => outln!("{{\"outputs\":[{}],\"diagnostics\":[]}}", paths.join(",")),
        }
    }
    ExitCode::SUCCESS
}

/// Print a refusal's `diagnostics`, already located, and return `code`.
pub fn refuse(diagnostics: &[Diagnostic], json: bool, code: ExitCode) -> ExitCode {
    if json {
        let entries: Vec<String> = diagnostics.iter().map(Diagnostic::to_json).collect();
        outln!(
            "{{\"output\":null,\"diagnostics\":[{}]}}",
            entries.join(",")
        );
    } else {
        print_diagnostics(diagnostics);
    }
    code
}

/// Write the executable to `path`, executable by everyone, as a compiler's
/// output is.
fn write_executable(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The refusal of an output that could not be written (`E0391`). The
/// reason is in words of its own, the same on every host. A directory is
/// checked first: Windows refuses to write one as "permission denied",
/// Linux as "is a directory".
fn unwritable(path: &str, err: &std::io::Error) -> Diagnostic {
    let reason = match err.kind() {
        _ if Path::new(path).is_dir() => "it is a directory".to_string(),
        std::io::ErrorKind::NotFound => "its directory does not exist".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => err.to_string(),
    };
    Diagnostic::new(
        oska_native::codes::UNWRITABLE_OUTPUT,
        format!("cannot write the executable `{path}`: {reason}; name another path with `-o`"),
        Span { start: 0, end: 0 },
    )
}
