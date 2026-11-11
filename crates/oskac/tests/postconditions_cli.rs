//! `oskac run` on a broken postcondition (#107, spec #105): `P0007` is an
//! ordinary panic, reported in the JSON shape of every panic, with exit
//! code 101, and `--release` evaluates contracts too.

use std::path::Path;
use std::process::Command;

const SRC: &str = "fn absolute(x: Int) -> Int
    ensures |r| r >= 0
{
    x
}

fn main() {
    let y = absolute(-1)
}
";

#[test]
fn a_broken_postcondition_under_release_is_a_json_panic_exiting_101() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("postcondition.osk");
    std::fs::write(&path, SRC).expect("should write the source file");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("run")
        .arg(&path)
        .args(["--release", "--format=json"])
        .output()
        .expect("oskac should run");
    let start = SRC.find("ensures").expect("the clause");
    let end = start + "ensures |r| r >= 0".len();
    assert_eq!(output.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "{{\"code\":\"P0007\",\"message\":\"postcondition violated: `absolute` ensures \
             `r >= 0`, broken by the return at line 4, column 5\",\
             \"path\":\"{}\",\"span\":{{\"start\":{start},\"end\":{end}}}}}\n",
            path.to_string_lossy().replace('\\', "\\\\")
        )
    );
}
