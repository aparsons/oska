// SPDX-License-Identifier: Apache-2.0
//! The build cache (spec #279): `oskac check`, `run`, `test` and `build`
//! reuse a result whose inputs are unchanged, and a reused result is
//! indistinguishable from a fresh one.
//!
//! One seam, the real `oskac`, with `OSKA_CACHE_DIR` pointed at a folder
//! of each case's own. The cases look at outputs, exit codes and `oskac
//! cache info`, never at how the cache lays out its files: the tampering
//! cases change every file under the cache folder alike.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::Json;

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";
const BYE: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"bye\")\n}\n";
const BROKEN: &str = "fn main() -> Int {\n    \"not an int\"\n}\n";
const TESTED: &str = "fn double(n: Int) -> Int {\n    n * 2\n}\n\ntest \"double doubles\" {\n    t.assert_eq(double(2), 4)\n}\n\ntest \"double is wrong here\" {\n    t.assert_eq(double(3), 7)\n}\n";

/// Whether `oskac run` and `oskac test` compile natively on this host,
/// and so use native entries; elsewhere they interpret.
const NATIVE_HOST: bool = cfg!(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
));

/// A case: a fresh folder holding `files`, and its own cache folder.
struct Case {
    dir: PathBuf,
    cache: PathBuf,
    /// The `oskac` to run: the one cargo built, unless a case copies it.
    oskac: PathBuf,
    env: Vec<(String, String)>,
}

impl Case {
    fn new(name: &str, files: &[(&str, &str)]) -> Case {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("build_cache")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("should create the case folder");
        let case = Case {
            cache: dir.join("cache"),
            dir,
            oskac: PathBuf::from(env!("CARGO_BIN_EXE_oskac")),
            env: Vec::new(),
        };
        for (path, text) in files {
            case.write(path, text);
        }
        case
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a folder");
        std::fs::write(&file, text).expect("should write the file");
    }

    /// Run `oskac args` in the case folder, with its cache.
    fn oskac(&self, args: &[&str]) -> Output {
        let mut command = Command::new(&self.oskac);
        command
            .args(args)
            .current_dir(&self.dir)
            .env("OSKA_CACHE_DIR", &self.cache)
            .env_remove("OSKA_CACHE")
            .env_remove("OSKA_CACHE_LIMIT");
        for (name, value) in &self.env {
            command.env(name, value);
        }
        common::portable::output_of(&mut command)
    }

    /// `oskac cache info --format=json`.
    fn info(&self) -> Json {
        let output = self.oskac(&["cache", "info", "--format=json"]);
        assert!(output.status.success(), "{}", text(&output.stderr));
        Json::parse(&text(&output.stdout))
    }

    fn count(&self, field: &str) -> u64 {
        match self.info().get(field) {
            Some(Json::Num(n)) => *n,
            other => panic!("cache info has no number {field}: {other:?}"),
        }
    }

    /// Run `oskac args` and say whether it was served from the cache: a
    /// hit counted and no miss.
    fn hit(&self, args: &[&str]) -> (Output, bool) {
        let (hits, misses) = (self.count("hits"), self.count("misses"));
        let output = self.oskac(args);
        let hit = self.count("hits") > hits && self.count("misses") == misses;
        (output, hit)
    }

    /// Every regular file under the cache folder.
    fn cache_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut folders = vec![self.cache.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).expect("should list the cache") {
                let path = entry.expect("an entry").path();
                let kind = std::fs::symlink_metadata(&path)
                    .expect("metadata")
                    .file_type();
                if kind.is_dir() {
                    folders.push(path);
                } else if kind.is_file() {
                    files.push(path);
                }
            }
        }
        files
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Two outputs alike: exit code, stdout and stderr.
fn assert_same(fresh: &Output, cached: &Output) {
    assert_eq!(fresh.status.code(), cached.status.code(), "exit code");
    assert_eq!(text(&fresh.stdout), text(&cached.stdout), "stdout");
    assert_eq!(text(&fresh.stderr), text(&cached.stderr), "stderr");
}

#[test]
fn a_second_check_is_a_hit_with_identical_output() {
    let case = Case::new("check", &[("ok.osk", HELLO), ("broken.osk", BROKEN)]);
    for file in ["ok.osk", "broken.osk"] {
        for format in ["--format=text", "--format=json"] {
            let args = ["check", file, format];
            let (fresh, hit) = case.hit(&args);
            assert!(!hit, "the first check of {file} {format} is a miss");
            let (cached, hit) = case.hit(&args);
            assert!(hit, "the second check of {file} {format} is a hit");
            assert_same(&fresh, &cached);
        }
    }
    let broken = case.oskac(&["check", "broken.osk"]);
    assert_eq!(
        broken.status.code(),
        Some(1),
        "diagnostics replay their code"
    );
    assert!(
        text(&broken.stderr).contains("broken.osk:"),
        "{}",
        text(&broken.stderr)
    );
}

#[test]
fn a_package_check_is_a_hit_and_a_change_to_any_file_misses() {
    let case = Case::new(
        "package",
        &[
            ("app/oska.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n"),
            ("app/src/main.osk", "use pkg.util\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    util.say(io)\n}\n"),
            ("app/src/util.osk", "pub(pkg) fn say(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n"),
        ],
    );
    let (fresh, _) = case.hit(&["check", "app"]);
    assert!(fresh.status.success(), "{}", text(&fresh.stderr));
    let (cached, hit) = case.hit(&["check", "app"]);
    assert!(hit);
    assert_same(&fresh, &cached);
    case.write(
        "app/src/util.osk",
        "pub(pkg) fn say(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n",
    );
    assert!(!case.hit(&["check", "app"]).1, "a changed module misses");
    case.write(
        "app/oska.toml",
        "[package]\nname = \"app\"\nversion = \"0.2.0\"\n",
    );
    assert!(!case.hit(&["check", "app"]).1, "a changed manifest misses");
    case.write("app/src/extra.osk", "pub(pkg) fn unused() {}\n");
    assert!(!case.hit(&["check", "app"]).1, "a new module misses");
    assert!(case.hit(&["check", "app"]).1);
}

#[test]
fn a_change_to_the_source_or_an_output_flag_misses() {
    let case = Case::new("flags", &[("main.osk", HELLO)]);
    case.oskac(&["check", "main.osk"]);
    assert!(case.hit(&["check", "main.osk"]).1);
    assert!(!case.hit(&["check", "main.osk", "--format=json"]).1);
    assert!(!case.hit(&["check", "main.osk", "--release"]).1);
    case.write("main.osk", BYE);
    assert!(
        !case.hit(&["check", "main.osk"]).1,
        "a changed module misses"
    );
    std::fs::copy(case.dir.join("main.osk"), case.dir.join("other.osk")).expect("a copy");
    assert!(
        !case.hit(&["check", "other.osk"]).1,
        "the same text by another path misses: the output names the path"
    );
}

#[test]
fn run_and_test_reuse_their_native_executables() {
    let case = Case::new("run", &[("main.osk", HELLO), ("tested.osk", TESTED)]);
    // The test executable reports unplaced whatever the format, so the
    // JSON run reuses the one the text run built.
    for (args, first_hits) in [
        (&["run", "main.osk"][..], false),
        (&["run", "main.osk", "--format=json"], false),
        (&["test", "tested.osk"], false),
        (&["test", "tested.osk", "--format=json"], NATIVE_HOST),
    ] {
        let (fresh, hit) = case.hit(args);
        assert_eq!(hit, first_hits, "{args:?}");
        let (cached, hit) = case.hit(args);
        assert_eq!(
            hit, NATIVE_HOST,
            "{args:?}: a hit only where it runs natively"
        );
        if args.ends_with(&["--format=json"]) && args[0] == "test" {
            // A test's `duration_ms` is the time it took, which no cache
            // keeps: compare the reports with it masked.
            let masked = |o: &Output| Output {
                status: o.status,
                stdout: common::golden::mask_durations(&text(&o.stdout)).into_bytes(),
                stderr: o.stderr.clone(),
            };
            assert_same(&masked(&fresh), &masked(&cached));
        } else {
            assert_same(&fresh, &cached);
        }
    }
    let tested = case.oskac(&["test", "tested.osk"]);
    assert_eq!(tested.status.code(), Some(1), "a failing test still fails");
}

#[test]
fn build_reuses_its_executable_and_a_target_change_misses() {
    let case = Case::new("build", &[("main.osk", HELLO)]);
    let build =
        |out: &str, target: &str| case.hit(&["build", "main.osk", "-o", out, "--target", target]);
    let (fresh, hit) = build("one", "linux-aarch64");
    assert!(fresh.status.success() && !hit, "{}", text(&fresh.stderr));
    let (cached, hit) = build("two", "linux-aarch64");
    assert!(hit);
    assert_same(&fresh, &cached);
    let read = |name: &str| std::fs::read(case.dir.join(name)).expect("the executable");
    assert_eq!(read("one"), read("two"), "byte-identical executables");
    let (_, hit) = build("three", "linux-x86_64");
    assert!(!hit, "another target misses");
    assert_ne!(read("one"), read("three"));
}

#[test]
fn a_changed_oskac_misses() {
    let mut case = Case::new("binary", &[("main.osk", HELLO)]);
    case.oskac(&["check", "main.osk"]);
    assert!(case.hit(&["check", "main.osk"]).1);
    // A copy is the same executable: its contents are what count.
    let copy = case
        .dir
        .join(format!("oskac-copy{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_oskac"), &copy).expect("a copy of oskac");
    case.oskac = copy.clone();
    assert!(case.hit(&["check", "main.osk"]).1, "an identical copy hits");
    // One more byte at the end: still runs, but is another build. It is
    // a new file, not the copy rewritten: macOS keeps the code signature
    // of an executable it has run with its file, and kills the process
    // when a file it has run changes in place.
    let mut bytes = std::fs::read(&copy).expect("the copy");
    bytes.push(0);
    let changed = case
        .dir
        .join(format!("oskac-changed{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&changed, bytes).expect("a changed copy");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&changed, std::fs::Permissions::from_mode(0o755))
            .expect("the changed copy is executable");
    }
    case.oskac = changed;
    let (output, hit) = case.hit(&["check", "main.osk"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(!hit, "a changed oskac misses");
}

#[test]
fn a_tampered_or_truncated_entry_is_rebuilt() {
    let case = Case::new("tampered", &[("main.osk", HELLO)]);
    let args = [
        "build",
        "main.osk",
        "-o",
        "out",
        "--target",
        "linux-aarch64",
    ];
    let fresh = case.oskac(&args);
    let executable = std::fs::read(case.dir.join("out")).expect("the executable");
    for damage in ["flip", "truncate"] {
        for file in case.cache_files() {
            let mut bytes = std::fs::read(&file).expect("a cache file");
            match damage {
                "flip" => {
                    if let Some(last) = bytes.last_mut() {
                        *last ^= 0x55;
                    }
                }
                _ => bytes.truncate(bytes.len() / 2),
            }
            std::fs::write(&file, bytes).expect("a damaged cache file");
        }
        let (rebuilt, hit) = case.hit(&args);
        assert!(!hit, "a {damage}ped entry is a miss");
        assert_same(&fresh, &rebuilt);
        assert_eq!(
            std::fs::read(case.dir.join("out")).expect("the executable"),
            executable,
            "a {damage}ped entry is never used"
        );
        assert!(case.hit(&args).1, "the rebuilt entry hits");
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_entry_is_never_followed() {
    let case = Case::new("symlinked", &[("main.osk", HELLO)]);
    let fresh = case.oskac(&["check", "main.osk"]);
    let elsewhere = case.dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a folder");
    for (n, file) in case.cache_files().into_iter().enumerate() {
        let target = elsewhere.join(n.to_string());
        std::fs::rename(&file, &target).expect("move the file out");
        std::os::unix::fs::symlink(&target, &file).expect("a symlink in its place");
    }
    let cached = case.oskac(&["check", "main.osk"]);
    assert_same(&fresh, &cached);
    assert_eq!(case.count("hits"), 0, "nothing was read through a symlink");
    assert!(case.hit(&["check", "main.osk"]).1, "the rebuilt entry hits");
}

#[cfg(unix)]
#[test]
fn a_cache_folder_that_is_a_symlink_or_writable_by_others_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let mut case = Case::new("refused", &[("main.osk", HELLO)]);
    let check = |case: &Case| {
        let output = case.oskac(&["check", "main.osk"]);
        assert!(output.status.success());
        assert!(
            text(&output.stderr).starts_with("warning: running without the build cache: "),
            "{}",
            text(&output.stderr)
        );
        output
    };

    // Writable by its group.
    std::fs::create_dir_all(&case.cache).expect("the cache folder");
    std::fs::set_permissions(&case.cache, std::fs::Permissions::from_mode(0o770)).expect("chmod");
    let output = check(&case);
    assert!(text(&output.stderr).contains("writable by its group or by others"));
    let info = case.info();
    assert_eq!(info.get("used"), Some(&Json::Bool(false)));
    assert_eq!(
        std::fs::read_dir(&case.cache).expect("list").count(),
        0,
        "nothing written"
    );

    // A symlink to a private folder.
    let real = case.dir.join("real");
    std::fs::create_dir(&real).expect("a folder");
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let link = case.dir.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("a symlink");
    case.cache = link;
    let output = check(&case);
    assert!(text(&output.stderr).contains("is a symbolic link"));
    assert_eq!(
        std::fs::read_dir(&real).expect("list").count(),
        0,
        "nothing written"
    );

    // Owned by another user: only root can make one to test with.
    let probe = case.dir.join("probe");
    std::fs::write(&probe, "").expect("a file");
    let root = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&probe).expect("meta")) == 0;
    if root {
        let foreign = case.dir.join("foreign");
        std::fs::create_dir(&foreign).expect("a folder");
        std::fs::set_permissions(&foreign, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        std::os::unix::fs::chown(&foreign, Some(65534), Some(65534)).expect("chown");
        case.cache = foreign;
        let output = check(&case);
        assert!(text(&output.stderr).contains("owned by another user"));
    }
}

#[test]
fn sixteen_parallel_runs_all_succeed_alike() {
    let case = Case::new("parallel", &[("main.osk", HELLO)]);
    let outputs: Vec<(Output, Vec<u8>)> = std::thread::scope(|scope| {
        let runs: Vec<_> = (0..16)
            .map(|n| {
                let case = &case;
                scope.spawn(move || {
                    let out = format!("out{n}");
                    let output =
                        case.oskac(&["build", "main.osk", "-o", &out, "--target", "linux-aarch64"]);
                    let bytes = std::fs::read(case.dir.join(&out)).unwrap_or_default();
                    (output, bytes)
                })
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("the run should not panic"))
            .collect()
    });
    let (first, executable) = &outputs[0];
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(!executable.is_empty());
    for (output, bytes) in &outputs {
        assert_same(first, output);
        assert_eq!(bytes, executable, "every run wrote the same executable");
    }
    let (_, hit) = case.hit(&[
        "build",
        "main.osk",
        "-o",
        "after",
        "--target",
        "linux-aarch64",
    ]);
    assert!(hit, "the entry the parallel runs wrote is whole");
}

#[test]
fn eviction_keeps_the_cache_under_its_limit() {
    let measure = Case::new("evict-measure", &[("main.osk", HELLO)]);
    let build = |case: &Case, file: &str| {
        let output = case.oskac(&["build", file, "-o", "out", "--target", "linux-aarch64"]);
        assert!(output.status.success(), "{}", text(&output.stderr));
    };
    build(&measure, "main.osk");
    let one = measure.count("size");

    let mut case = Case::new("evict", &[("a.osk", HELLO), ("b.osk", BYE)]);
    let limit = one + one / 2;
    case.env
        .push(("OSKA_CACHE_LIMIT".to_string(), limit.to_string()));
    build(&case, "a.osk");
    assert!(case.count("size") <= limit);
    build(&case, "b.osk");
    assert!(
        case.count("size") <= limit,
        "the older program's entry is evicted"
    );
    assert_eq!(case.count("limit"), limit);
    let args = ["build", "a.osk", "-o", "out", "--target", "linux-aarch64"];
    assert!(!case.hit(&args).1, "the evicted entry misses");
    assert!(case.hit(&args).1, "and is stored again");
    assert!(case.count("size") <= limit);
}

#[test]
fn no_cache_and_oska_cache_off_leave_the_cache_untouched() {
    let mut case = Case::new("off", &[("main.osk", HELLO)]);
    let fresh = case.oskac(&["check", "main.osk", "--no-cache"]);
    assert!(fresh.status.success(), "{}", text(&fresh.stderr));
    assert!(!case.cache.exists(), "--no-cache makes no cache");
    case.oskac(&["build", "main.osk", "--no-cache", "-o", "out"]);
    assert!(!case.cache.exists());

    case.oskac(&["check", "main.osk"]);
    let before = (
        case.count("entries"),
        case.count("hits"),
        case.count("misses"),
    );
    let output = case.oskac(&["check", "main.osk", "--no-cache"]);
    assert_same(&fresh, &output);
    case.env.push(("OSKA_CACHE".to_string(), "off".to_string()));
    let output = case.oskac(&["check", "main.osk"]);
    assert_same(&fresh, &output);
    let info = case.info();
    assert_eq!(info.get("used"), Some(&Json::Bool(false)));
    case.env.clear();
    let after = (
        case.count("entries"),
        case.count("hits"),
        case.count("misses"),
    );
    assert_eq!(before, after, "neither read nor wrote the cache");

    let usage = case.oskac(&["lex", "main.osk", "--no-cache"]);
    assert_eq!(
        usage.status.code(),
        Some(2),
        "only check, run, test and build take it"
    );
}

#[test]
fn cache_clean_empties_it() {
    let case = Case::new("clean", &[("main.osk", HELLO)]);
    case.oskac(&["check", "main.osk"]);
    case.oskac(&["check", "main.osk"]);
    assert!(case.count("entries") > 0);
    let cleaned = case.oskac(&["cache", "clean"]);
    assert!(cleaned.status.success(), "{}", text(&cleaned.stderr));
    assert!(
        text(&cleaned.stdout).starts_with("removed "),
        "{}",
        text(&cleaned.stdout)
    );
    let info = case.info();
    for field in ["entries", "size", "hits", "misses"] {
        assert_eq!(info.get(field), Some(&Json::Num(0)), "{field}");
    }
    assert!(!case.hit(&["check", "main.osk"]).1, "a clean cache misses");
}

#[test]
fn cache_info_reports_the_folder() {
    let case = Case::new("info", &[]);
    let human = case.oskac(&["cache", "info"]);
    assert!(human.status.success());
    let shown = case.cache.display().to_string();
    assert!(
        text(&human.stdout).starts_with(&format!("location: {shown}\nused: yes\nentries: 0\n")),
        "{}",
        text(&human.stdout)
    );
    assert!(!case.cache.exists(), "info makes nothing");
    let info = case.info();
    assert_eq!(info.get("location"), Some(&Json::Str(shown)));
    assert_eq!(info.get("limit"), Some(&Json::Num(1 << 30)));
    let usage = case.oskac(&["cache", "nope"]);
    assert_eq!(usage.status.code(), Some(2));
}
