//! Tests of the portable test helpers in `common/portable.rs`.

mod common;

use common::portable::*;
use common::Json;
use std::path::Path;
use std::process::Command;

#[test]
fn scratch_is_empty_canonical_and_removed_on_drop() {
    let path = {
        let scratch = Scratch::new("oskac-portable", "scratch");
        assert!(scratch.0.is_dir());
        assert_eq!(scratch.0, scratch.0.canonicalize().expect("io"));
        assert_eq!(std::fs::read_dir(&scratch.0).expect("io").count(), 0);
        scratch.0.clone()
    };
    assert!(!path.exists());
}

#[test]
fn json_str_escapes_backslashes_and_quotes() {
    assert_eq!(json_str(r#"C:\a\"b""#), r#""C:\\a\\\"b\"""#);
    assert_eq!(json_escape(r"\\?\C:\x"), r"\\\\?\\C:\\x");
}

#[test]
fn json_path_round_trips_through_a_json_reader() {
    let path = Path::new(if cfg!(windows) {
        r"\\?\C:\Users\me\.oska"
    } else {
        "/home/me/.oska"
    });
    let parsed = Json::parse(&json_path(path));
    assert_eq!(parsed, Json::Str(path.to_string_lossy().into_owned()));
}

#[test]
fn literal_path_keeps_dot_dot() {
    let base = Path::new(if cfg!(windows) { r"\\?\C:\p" } else { "/p" });
    let joined = literal_path(base, &["x", "..", ".claude"]);
    let text = joined.to_string_lossy().into_owned();
    assert!(text.ends_with(&format!("x{SEP}..{SEP}.claude")), "{text}");
    assert!(joined.components().any(|c| c.as_os_str() == ".."));
}

#[test]
fn set_home_sets_every_home_variable() {
    let scratch = Scratch::new("oskac-portable", "home");
    let mut command = Command::new("unused");
    set_home(&mut command, &scratch.0);
    let envs: Vec<_> = command
        .get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(Path::new)))
        .collect();
    let get = |name: &str| envs.iter().find(|(k, _)| k == name).and_then(|(_, v)| *v);
    assert_eq!(get("HOME"), Some(scratch.0.as_path()));
    assert_eq!(get("USERPROFILE"), Some(scratch.0.as_path()));
    assert_eq!(
        get("XDG_CONFIG_HOME"),
        Some(scratch.0.join(".config").as_path())
    );
}
