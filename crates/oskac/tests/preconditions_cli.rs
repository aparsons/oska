//! `oskac run` on a broken precondition (#106, spec #105): `P0006` is an
//! ordinary panic, reported in the JSON shape of every panic, with exit
//! code 101, and `--release` evaluates contracts too.

use std::path::Path;
use std::process::Command;

const SRC: &str = "fn double(x: Int) -> Int
    requires x > 0
{
    x * 2
}

fn main() {
    let y = double(-1)
}
";

#[test]
fn a_broken_precondition_under_release_is_a_json_panic_exiting_101() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("precondition.osk");
    std::fs::write(&path, SRC).expect("should write the source file");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("run")
        .arg(&path)
        .args(["--release", "--format=json"])
        .output()
        .expect("oskac should run");
    let start = SRC.find("double(-1)").expect("the call");
    let end = start + "double(-1)".len();
    assert_eq!(output.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "{{\"code\":\"P0006\",\"message\":\"precondition violated: `double` requires `x > 0`\",\
             \"path\":\"{}\",\"span\":{{\"start\":{start},\"end\":{end}}}}}\n",
            path.to_string_lossy().replace('\\', "\\\\")
        )
    );
}
