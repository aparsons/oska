//! The whole filesystem, as ADR 0008 spells it (#99): `Fs`, `FsRead` and
//! `FsWrite` grant no file access, so a program that really needs to read
//! anywhere writes `restrict_to("/")` and works through the directory
//! capability it gets.

mod common;

use common::{assert_runs, run_on};
use oska_run::FakeHost;

#[test]
fn restrict_to_slash_reaches_files_anywhere_on_the_fake_host() {
    let host = FakeHost::new()
        .with_file("etc/app.conf", "mode=fast")
        .with_file("srv/data/one.txt", "1");
    let src = r#"fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let everything = fs.restrict_to("/")
    io.println(everything.read_only().read_to_string(Path.new("etc/app.conf"))?)?
    let _ = everything.write_only().write(Path.new("srv/data/two.txt"), "2")?
    for name in everything.read_only().list_dir(Path.new("srv/data"))? {
        io.println(name)?
    }
    Ok(())
}
"#;
    let outcome = run_on(host, src, &[]);
    assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
    assert_eq!(outcome.stdout, "mode=fast\none.txt\ntwo.txt\n");
}

#[test]
fn the_whole_filesystem_is_still_confined_by_the_path_rules() {
    let src = r#"fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let everything = fs.restrict_to("/")
    match everything.read_to_string(Path.new("../up.txt")) {
        Ok(text) => io.println(text)
        Err(e) => io.println("refused")
    }
}
"#;
    assert_runs(src, &[], "refused\n");
}

#[cfg(unix)]
mod real {
    use oska_run::{run, RealHost};

    #[test]
    fn restrict_to_slash_reads_and_writes_a_real_file() {
        let base = std::env::temp_dir().join(format!("oska-run-whole-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("create the sandbox");
        let base = std::fs::canonicalize(&base).expect("canonicalize the sandbox");
        std::fs::write(base.join("in.txt"), "from disk").expect("write the input");
        let relative = base.strip_prefix("/").expect("an absolute path");
        let src = format!(
            "fn main(fs: Fs) -> Result<(), IoError> {{\n    \
             let everything = fs.restrict_to(\"/\")\n    \
             let text = everything.read_only().read_to_string(Path.new(\"{0}/in.txt\"))?\n    \
             everything.write_only().write(Path.new(\"{0}/out.txt\"), text)\n}}\n",
            relative.display()
        );
        let outcome = run(&src, &mut RealHost::new(), &["prog.osk".to_string()]);
        assert_eq!(outcome.exit_code, 0, "{outcome:#?}");
        let written = std::fs::read_to_string(base.join("out.txt")).expect("written");
        std::fs::remove_dir_all(&base).expect("clean up");
        assert_eq!(written, "from disk");
    }
}
