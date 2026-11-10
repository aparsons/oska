//! Race-free confined writes, `create_dir` and `list_dir` (#95): through a
//! directory capability, each walks its path one component at a time
//! beneath a handle for the root, never following a symlink, as reads do
//! (#94, `confined_reads.rs`).
//!
//! Every scenario runs an Oska program twice, against the same layout:
//!
//! - on [`RealHost`] against a real temporary directory with real
//!   symlinks, on Unix only: the escape tests. A symlink planted at a
//!   component stands for one swapped in there at any moment of the walk,
//!   which refuses it either way, so the race is tested without timing.
//! - on [`FakeHost`], on every OS: the same refusals, so API tests that
//!   use the fake host behave as the real one does.

use std::path::Path;

use oska_run::{run, FakeHost, Host};

/// A filesystem laid out for every scenario, relative to a base
/// directory:
///
/// ```text
/// root/inside.txt         "inside text"
/// root/sub/nested.txt     "nested text"
/// outside/secret.txt      "outside text"
/// ```
trait World {
    /// The absolute path of `at`, relative to the base.
    fn path(&self, at: &str) -> String;

    /// Plant a symlink at `at` pointing to `target`, both relative to the
    /// base. The link holds `target`'s absolute path.
    fn link(&mut self, at: &str, target: &str);

    /// Run `src`, and return what it printed.
    fn run(&mut self, src: &str) -> String;

    /// The text of the file at `at`, relative to the base, if there is
    /// one.
    fn file(&mut self, at: &str) -> Option<String>;

    /// Whether a directory is at `at`.
    fn is_dir(&mut self, at: &str) -> bool;

    /// The names in `outside`, sorted.
    fn outside_names(&mut self) -> Vec<String>;
}

/// A program body that runs `call`, then runs `ok` with its `Ok` value
/// bound to `value` after printing `ok`, or prints `REFUSED: ` and the
/// `IoError`'s message.
fn report(call: &str, ok: &str) -> String {
    format!(
        "    match {call} {{\n\
         \x20       Ok(value) => {{\n\
         \x20           let _ = io.println(\"ok\")\n\
         {ok}\
         \x20       }}\n\
         \x20       Err(e) => {{\n\
         \x20           let message = e.message\n\
         \x20           let _ = io.println(\"REFUSED: {{message}}\")\n\
         \x20       }}\n\
         \x20   }}\n"
    )
}

/// What a program prints when it writes `contents` to `path` through a
/// filesystem restricted to `root`.
fn write(world: &mut dyn World, root: &str, path: &str, contents: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: FsWrite) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n{}}}\n",
        world.path(root),
        report(
            &format!("local.write(Path.new(\"{path}\"), \"{contents}\")"),
            "            let _ = value\n"
        )
    );
    world.run(&src)
}

/// What a program prints when it creates the directory `path` through a
/// filesystem restricted to `root`.
fn create_dir(world: &mut dyn World, root: &str, path: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: FsWrite) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n{}}}\n",
        world.path(root),
        report(
            &format!("local.create_dir(Path.new(\"{path}\"))"),
            "            let _ = value\n"
        )
    );
    world.run(&src)
}

/// What a program prints when it lists `path` through a filesystem
/// restricted to `root`: `ok`, then the names, a line each.
fn list_dir(world: &mut dyn World, root: &str, path: &str) -> String {
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n{}}}\n",
        world.path(root),
        report(
            &format!("local.list_dir(Path.new(\"{path}\"))"),
            "            for name in value {\n\
             \x20               let _ = io.println(name)\n\
             \x20           }\n"
        )
    );
    world.run(&src)
}

/// Assert that `printed` is a refusal whose message contains `why`.
#[track_caller]
fn assert_refused(printed: &str, why: &str) {
    assert!(
        printed.starts_with("REFUSED: ") && printed.contains(why),
        "should be refused for `{why}`, printed {printed:?}"
    );
}

// ---- The real host: a temporary directory -----------------------------------

#[cfg(unix)]
mod real {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use oska_run::{Output, RealHost, RootHandle};

    use super::*;

    pub struct Real {
        base: PathBuf,
    }

    impl Real {
        pub fn new(test: &str) -> Real {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let unique = format!(
                "oska-writes-{test}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let base = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(base.join("root/sub")).expect("create the root");
            std::fs::create_dir_all(base.join("outside")).expect("create the outside");
            std::fs::write(base.join("root/inside.txt"), "inside text").expect("write");
            std::fs::write(base.join("root/sub/nested.txt"), "nested text").expect("write");
            std::fs::write(base.join("outside/secret.txt"), "outside text").expect("write");
            let base = std::fs::canonicalize(&base).expect("canonicalize the base");
            Real { base }
        }
    }

    impl Drop for Real {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    impl World for Real {
        fn path(&self, at: &str) -> String {
            self.base.join(at).display().to_string()
        }

        fn link(&mut self, at: &str, target: &str) {
            std::os::unix::fs::symlink(self.base.join(target), self.base.join(at))
                .expect("create a symlink");
        }

        fn run(&mut self, src: &str) -> String {
            let mut host = Recording {
                real: RealHost::new(),
                stdout: String::new(),
            };
            let outcome = run(src, &mut host, &["prog.osk".to_string()]);
            assert!(
                outcome.diagnostics.is_empty() && outcome.panic.is_none(),
                "{src} should run: {outcome:#?}"
            );
            outcome.stdout.trim_end().to_string()
        }

        fn file(&mut self, at: &str) -> Option<String> {
            let path = self.base.join(at);
            let there = std::fs::symlink_metadata(&path).ok()?;
            there
                .is_file()
                .then(|| std::fs::read_to_string(path).expect("read"))
        }

        fn is_dir(&mut self, at: &str) -> bool {
            std::fs::symlink_metadata(self.base.join(at)).is_ok_and(|m| m.is_dir())
        }

        fn outside_names(&mut self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(self.base.join("outside"))
                .expect("list the outside")
                .map(|entry| {
                    entry
                        .expect("an entry")
                        .file_name()
                        .into_string()
                        .expect("UTF-8")
                })
                .collect();
            names.sort();
            names
        }
    }

    /// [`RealHost`] with its stdout recorded. Every other operation goes
    /// straight to the real OS.
    struct Recording {
        real: RealHost,
        stdout: String,
    }

    impl Host for Recording {
        fn write_stdout(&mut self, text: &str) -> std::io::Result<()> {
            self.stdout.push_str(text);
            Ok(())
        }
        fn write_stderr(&mut self, text: &str) -> std::io::Result<()> {
            self.real.write_stderr(text)
        }
        fn read_line(&mut self) -> std::io::Result<Option<String>> {
            self.real.read_line()
        }
        fn env_var(&mut self, name: &str) -> Option<String> {
            self.real.env_var(name)
        }
        fn now_millis(&mut self) -> i128 {
            self.real.now_millis()
        }
        fn next_random(&mut self) -> u64 {
            self.real.next_random()
        }
        fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
            self.real.open_root(path)
        }
        fn open_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<RootHandle> {
            self.real.open_beneath(root, names)
        }
        fn open_file_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
            self.real.open_file_root(path)
        }
        fn open_file_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<RootHandle> {
            self.real.open_file_beneath(root, names)
        }
        fn read_file_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<String> {
            self.real.read_file_beneath(root, names)
        }
        fn write_file_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
            contents: &str,
        ) -> std::io::Result<()> {
            self.real.write_file_beneath(root, names, contents)
        }
        fn create_dir_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<()> {
            self.real.create_dir_beneath(root, names)
        }
        fn list_dir_beneath(
            &mut self,
            root: &RootHandle,
            names: &[String],
        ) -> std::io::Result<Vec<String>> {
            self.real.list_dir_beneath(root, names)
        }
        fn printed(&self) -> Vec<String> {
            self.stdout.lines().map(str::to_string).collect()
        }
        fn take_output(&mut self) -> Output {
            Output {
                stdout: std::mem::take(&mut self.stdout),
                stderr: String::new(),
            }
        }
    }
}

// ---- The fake host ---------------------------------------------------------------

mod fake {
    use super::*;

    /// The base directory of the fake layout.
    const BASE: &str = "/base";

    pub struct Fake {
        host: FakeHost,
    }

    impl Fake {
        pub fn new(_test: &str) -> Fake {
            let host = FakeHost::new()
                .with_file("base/root/inside.txt", "inside text")
                .with_file("base/root/sub/nested.txt", "nested text")
                .with_file("base/outside/secret.txt", "outside text");
            Fake { host }
        }
    }

    impl World for Fake {
        fn path(&self, at: &str) -> String {
            format!("{BASE}/{at}")
        }

        fn link(&mut self, at: &str, target: &str) {
            let host = std::mem::take(&mut self.host);
            self.host = host.with_symlink(&self.path(at), &self.path(target));
        }

        fn run(&mut self, src: &str) -> String {
            let outcome = run(src, &mut self.host, &["prog.osk".to_string()]);
            assert!(
                outcome.diagnostics.is_empty() && outcome.panic.is_none(),
                "{src} should run: {outcome:#?}"
            );
            outcome.stdout.trim_end().to_string()
        }

        fn file(&mut self, at: &str) -> Option<String> {
            self.host.file(&self.path(at)).map(str::to_string)
        }

        fn is_dir(&mut self, at: &str) -> bool {
            let path = self.path(at);
            self.host.open_root(Path::new(&path)).is_ok()
        }

        fn outside_names(&mut self) -> Vec<String> {
            let path = self.path("outside");
            let outside = self
                .host
                .open_root(Path::new(&path))
                .expect("open the outside");
            self.host
                .list_dir_beneath(&outside, &[])
                .expect("list the outside")
        }
    }
}

/// Each scenario, as a test on the fake host and, on Unix, the real one.
macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        mod on_fake {
            $(
                #[test]
                fn $name() {
                    super::$name(&mut super::fake::Fake::new(stringify!($name)));
                }
            )*
        }
        #[cfg(unix)]
        mod on_real {
            $(
                #[test]
                fn $name() {
                    super::$name(&mut super::real::Real::new(stringify!($name)));
                }
            )*
        }
    };
}

scenarios! {
    a_write_creates_or_replaces_a_file_beneath_the_root,
    a_write_through_a_final_component_symlink_creates_nothing,
    a_write_through_an_intermediate_symlink_is_refused,
    a_write_above_the_root_is_refused,
    a_write_that_does_not_end_in_a_name_is_refused,
    create_dir_makes_a_directory_and_keeps_one_already_there,
    create_dir_through_a_symlink_at_each_position_is_refused,
    create_dir_above_the_root_or_without_a_parent_is_refused,
    list_dir_lists_through_the_directory_sorted,
    list_dir_through_a_symlink_at_each_position_is_refused,
    list_dir_above_the_root_is_refused,
    a_symlinked_root_writes_creates_and_lists_nothing,
}

/// Every path beneath `root` and `outside` that holds a symlink to
/// `outside`, a dangling one into it, or one inside the root, for the
/// escape scenarios.
fn plant_links(world: &mut dyn World) {
    world.link("root/out", "outside");
    world.link("root/sub/out", "outside");
    world.link("root/alias", "root/sub");
    world.link("root/escape.txt", "outside/secret.txt");
    world.link("root/plant.txt", "outside/planted.txt");
    world.link("root/sub/plant.txt", "outside/planted.txt");
    world.link("root/alias.txt", "root/inside.txt");
    world.link("root/dangling", "outside/planted");
}

/// Assert that nothing was created or changed outside the root.
#[track_caller]
fn assert_outside_untouched(world: &mut dyn World) {
    assert_eq!(world.outside_names(), ["secret.txt"]);
    assert_eq!(
        world.file("outside/secret.txt").as_deref(),
        Some("outside text")
    );
}

// ---- write ---------------------------------------------------------------------------

fn a_write_creates_or_replaces_a_file_beneath_the_root(world: &mut dyn World) {
    assert_eq!(write(world, "root", "sub/new.txt", "fresh"), "ok");
    assert_eq!(world.file("root/sub/new.txt").as_deref(), Some("fresh"));
    // Shorter than what was there, so the file must have been emptied.
    assert_eq!(write(world, "root", "inside.txt", "again"), "ok");
    assert_eq!(world.file("root/inside.txt").as_deref(), Some("again"));
    assert_eq!(write(world, "root", "sub/../top.txt", "top"), "ok");
    assert_eq!(world.file("root/top.txt").as_deref(), Some("top"));
}

fn a_write_through_a_final_component_symlink_creates_nothing(world: &mut dyn World) {
    plant_links(world);
    for path in ["plant.txt", "sub/plant.txt", "escape.txt", "alias.txt"] {
        assert_refused(&write(world, "root", path, "pwned"), "symlink");
    }
    assert_outside_untouched(world);
    assert_eq!(
        world.file("root/inside.txt").as_deref(),
        Some("inside text")
    );
}

fn a_write_through_an_intermediate_symlink_is_refused(world: &mut dyn World) {
    plant_links(world);
    for path in [
        "out/new.txt",
        "sub/out/new.txt",
        "alias/new.txt",
        "out/x/../new.txt",
    ] {
        assert_refused(&write(world, "root", path, "pwned"), "symlink");
    }
    assert_outside_untouched(world);
    assert_eq!(world.file("root/sub/new.txt"), None);
}

fn a_write_above_the_root_is_refused(world: &mut dyn World) {
    for path in ["../outside/new.txt", "sub/../../outside/new.txt"] {
        assert_refused(
            &write(world, "root", path, "pwned"),
            "outside the capability's root",
        );
    }
    let absolute = world.path("outside/new.txt");
    assert_refused(&write(world, "root", &absolute, "pwned"), "absolute");
    assert_refused(&write(world, "root", "", "pwned"), "empty");
    assert_outside_untouched(world);
}

fn a_write_that_does_not_end_in_a_name_is_refused(world: &mut dyn World) {
    plant_links(world);
    for path in ["new/", "sub/.", ".", "sub/..", "dangling/", "dangling/."] {
        assert_refused(
            &write(world, "root", path, "x"),
            "does not end in a file name",
        );
    }
    assert_eq!(world.file("root/new"), None);
    assert_outside_untouched(world);
}

// ---- create_dir ------------------------------------------------------------------------

fn create_dir_makes_a_directory_and_keeps_one_already_there(world: &mut dyn World) {
    for path in ["new", "sub/deeper", "sub/../again", "sub"] {
        assert_eq!(create_dir(world, "root", path), "ok", "creating `{path}`");
    }
    for made in ["root/new", "root/sub/deeper", "root/again"] {
        assert!(world.is_dir(made), "`{made}` was not made");
    }
    assert_eq!(
        world.file("root/sub/nested.txt").as_deref(),
        Some("nested text")
    );
}

fn create_dir_through_a_symlink_at_each_position_is_refused(world: &mut dyn World) {
    plant_links(world);
    for path in [
        "out/new",
        "sub/out/new",
        "alias/new",
        "dangling",
        "alias",
        "out",
    ] {
        assert_refused(&create_dir(world, "root", path), "symlink");
    }
    assert_outside_untouched(world);
    assert!(!world.is_dir("root/sub/new"));
}

fn create_dir_above_the_root_or_without_a_parent_is_refused(world: &mut dyn World) {
    for path in ["..", "../outside/new", "sub/../../outside/new"] {
        assert_refused(
            &create_dir(world, "root", path),
            "outside the capability's root",
        );
    }
    let absolute = world.path("outside/new");
    assert_refused(&create_dir(world, "root", &absolute), "absolute");
    assert_refused(&create_dir(world, "root", ""), "empty");
    assert!(create_dir(world, "root", "missing/child").starts_with("REFUSED: "));
    assert!(create_dir(world, "root", "inside.txt").starts_with("REFUSED: "));
    assert!(!world.is_dir("root/missing"));
    assert_eq!(
        world.file("root/inside.txt").as_deref(),
        Some("inside text")
    );
    assert_outside_untouched(world);
}

// ---- list_dir ----------------------------------------------------------------------

fn list_dir_lists_through_the_directory_sorted(world: &mut dyn World) {
    world.link("root/out", "outside");
    assert_eq!(write(world, "root", "Zebra.txt", "z"), "ok");
    // A symlink is listed by its own name, and not followed.
    assert_eq!(
        list_dir(world, "root", "."),
        "ok\nZebra.txt\ninside.txt\nout\nsub"
    );
    for path in ["sub", "sub/.", "sub/../sub"] {
        assert_eq!(list_dir(world, "root", path), "ok\nnested.txt", "`{path}`");
    }
    // Listing twice through the same handle starts at the start each time.
    let src = format!(
        "fn main(io: Stdout, fs: FsRead) {{\n\
         \x20   let local = fs.restrict_to(\"{}\")\n\
         \x20   let first = local.list_dir(Path.new(\".\")).unwrap_or([]).len()\n\
         \x20   let second = local.list_dir(Path.new(\".\")).unwrap_or([]).len()\n\
         \x20   let _ = io.println(\"{{first}} {{second}}\")\n\
         }}\n",
        world.path("root/sub")
    );
    assert_eq!(world.run(&src), "1 1");
}

fn list_dir_through_a_symlink_at_each_position_is_refused(world: &mut dyn World) {
    plant_links(world);
    for path in ["out", "sub/out", "alias", "alias/.", "out/x/..", "dangling"] {
        assert_refused(&list_dir(world, "root", path), "symlink");
    }
}

fn list_dir_above_the_root_is_refused(world: &mut dyn World) {
    for path in ["..", "../outside", "sub/../.."] {
        assert_refused(
            &list_dir(world, "root", path),
            "outside the capability's root",
        );
    }
    let absolute = world.path("outside");
    assert_refused(&list_dir(world, "root", &absolute), "absolute");
    assert_refused(&list_dir(world, "root", ""), "empty");
    assert!(list_dir(world, "root", "inside.txt").starts_with("REFUSED: "));
    assert!(list_dir(world, "root", "missing").starts_with("REFUSED: "));
}

// ---- The root ------------------------------------------------------------------------------

fn a_symlinked_root_writes_creates_and_lists_nothing(world: &mut dyn World) {
    world.link("root-link", "root");
    world.link("outside-link", "outside");
    assert_refused(&write(world, "root-link", "new.txt", "x"), "symlink");
    assert_refused(&create_dir(world, "root-link", "new"), "symlink");
    assert_refused(&list_dir(world, "root-link", "."), "symlink");
    assert_refused(&write(world, "outside-link", "new.txt", "x"), "symlink");
    assert_eq!(world.file("root/new.txt"), None);
    assert!(!world.is_dir("root/new"));
    assert_outside_untouched(world);
}
