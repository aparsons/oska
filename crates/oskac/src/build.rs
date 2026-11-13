//! `oskac build`: check a source file or package and compile it with the
//! native backend (`oska-native`) to an executable for aarch64 Linux,
//! whatever the host.
//!
//! The output is the file's stem, or the package directory's name, in the
//! current directory, unless `-o` names it. Success prints nothing, or
//! `{"output": path, "diagnostics": []}` with `--format=json`. A refusal
//! prints its diagnostics as `oskac check` does, or as `{"output": null,
//! "diagnostics": [...]}`. The exit code is 1 for the checker's
//! diagnostics and an unwritable output (`E0391`), and 2 for a program
//! the backend cannot compile yet (`E0371`, `E0390`), as `oskac run`'s
//! is for one the interpreter cannot run.

use std::path::Path;
use std::process::ExitCode;

use oska_check::program::Program;
use oska_syntax::{json_escape, Diagnostic, Span};

use crate::{print_diagnostics, read_source, Flags, Places};

/// `oskac build` on the source file `path`.
pub fn file(path: &str, Flags { json, .. }: Flags, output: Option<&str>) -> ExitCode {
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
    compile(program, &places, output.unwrap_or(&stem), json)
}

/// `oskac build` on the package in `dir`, which checked as `program`.
pub fn package(
    program: &Program,
    places: &Places,
    dir: &str,
    output: Option<&str>,
    json: bool,
) -> ExitCode {
    let name = std::fs::canonicalize(dir)
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "main".to_string());
    compile(program, places, output.unwrap_or(&name), json)
}

/// Compile `program` and write it to `output`.
fn compile(program: &Program, places: &Places, output: &str, json: bool) -> ExitCode {
    let bytes = match oska_native::build(program, &|d| places.locate(d)) {
        Ok(bytes) => bytes,
        Err(refusal) => return refuse(&[places.locate(refusal)], json, ExitCode::from(2)),
    };
    if let Err(err) = write_executable(output, &bytes) {
        return refuse(&[unwritable(output, &err)], json, ExitCode::FAILURE);
    }
    if json {
        outln!(
            "{{\"output\":\"{}\",\"diagnostics\":[]}}",
            json_escape(output)
        );
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
