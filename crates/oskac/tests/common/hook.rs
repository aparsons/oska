//! What the hook tests share: scratch directories, Claude Code's hook
//! input, and running `oskac hook claude` the way Claude Code runs it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A new, empty directory for a test, under the system's temporary one,
/// removed when the test ends, even when it fails: one test copies `oskac`
/// in, and the copies filled `/tmp` when they were left behind.
pub fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("oska-agent-setup-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create a scratch directory");
    Scratch(dir)
}

/// A scratch directory, removed on drop.
pub struct Scratch(pub PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a hook run left behind: its exit code, standard output and
/// standard error.
pub struct Hooked {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Hooked {
    /// The lines of standard error.
    pub fn lines(&self) -> Vec<&str> {
        self.stderr.lines().collect()
    }
}

/// `oskac hook claude --event=post-edit`: Claude Code's tool-call JSON on
/// standard input, run in `cwd`, with no shell between: this is what the
/// published settings run, on every platform.
pub fn post_edit(input: &str, cwd: &Path) -> Hooked {
    hook(&["hook", "claude", "--event=post-edit"], input, cwd)
}

pub fn hook(args: &[&str], input: &str, cwd: &Path) -> Hooked {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oskac should run");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a pipe");
        // A hook that has already refused the input need not read it all.
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().expect("the hook should end");
    Hooked {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// `text` as a JSON string literal.
pub fn json_string(text: &str) -> String {
    format!("\"{}\"", oska_syntax::json_escape(text))
}

/// The JSON Claude Code gives a PostToolUse hook after an `Edit` of
/// `file`, in the project at `cwd`.
pub fn tool_call(file: &str, cwd: &Path) -> String {
    format!(
        "{{\"session_id\":\"s\",\"cwd\":{},\"hook_event_name\":\"PostToolUse\",\
         \"tool_name\":\"Edit\",\"tool_input\":{{\"file_path\":{},\
         \"old_string\":\"a\",\"new_string\":\"b\"}},\"tool_response\":{{\"success\":true}}}}",
        json_string(&cwd.to_string_lossy()),
        json_string(file)
    )
}

/// The call after an edit of `file`, a file under `cwd`.
pub fn edit_of(file: &Path, cwd: &Path) -> String {
    tool_call(&file.to_string_lossy(), cwd)
}

/// `files`, each a path under `dir` and its text, written there.
pub fn write_all(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("should create");
        std::fs::write(file, text).expect("should write");
    }
}

pub const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n";

pub const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// A symlink at `at` to the file `target`, or `false`, saying why, where
/// this OS or user may not make one: Windows without the privilege or
/// developer mode.
pub fn symlink_file(target: &Path, at: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, at);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    match made {
        Ok(()) => true,
        Err(err) => {
            println!("skipped: cannot make a symlink here: {err}");
            false
        }
    }
}

/// Every file under `dir`, by path, with its bytes.
pub fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.insert(path.clone(), std::fs::read(&path).expect("readable"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}
