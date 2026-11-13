// Prints one of the two Unicode tables in crates/oska-check/src/std/rt.osk,
// as the Rust that compiles this file knows Unicode. scripts/bump-rust.sh
// builds it with `rustc +<version>` and splices the output into rt.osk.
//
//   tables escaped     dbg's `escaped_table`: ranges of 12 hex digits
//   tables lowercase   `lowercase_table`: records of 13 hex digits
//
// The formats are documented above each table in rt.osk and checked, for
// every character, by `the_dbg_escape_table_is_rusts_for_every_character`
// (native_process.rs) and `the_lowercase_table_is_rusts_for_every_character`
// (native_text.rs).

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("escaped") => print!("{}", escaped()),
        Some("lowercase") => print!("{}", lowercase()),
        _ => {
            eprintln!("usage: tables escaped|lowercase");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

// The characters `{:?}` writes as `\u{hex}`, as runs: first and last, 6 digits each.
fn escaped() -> String {
    let mut out = String::new();
    let mut open: Option<(u32, u32)> = None;
    for cp in 0..=0x10FFFFu32 {
        let escaped = char::from_u32(cp)
            .is_some_and(|c| format!("{:?}", c.to_string())[1..].starts_with("\\u{"));
        open = match (escaped, open) {
            (true, None) => Some((cp, cp)),
            (true, Some((first, _))) => Some((first, cp)),
            (false, Some((first, last))) => {
                out.push_str(&format!("{first:06x}{last:06x}"));
                None
            }
            (false, None) => None,
        };
    }
    if let Some((first, last)) = open {
        out.push_str(&format!("{first:06x}{last:06x}"));
    }
    out
}

// Every character that has a one-character lower case other than itself
// (U+0130 is left to the runtime), as records: the first character (5
// digits), how many (2), the step between them (1) and the distance to the
// lower case plus 0x40000 (5). A record covers a contiguous run of the
// sorted mapped characters, because the lookup takes the last record that
// starts at or below a character.
fn lowercase() -> String {
    let mut mapped: Vec<(i64, i64)> = Vec::new();
    for cp in 0..=0x10FFFFu32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        if cp == 0x130 {
            continue;
        }
        let lower: Vec<char> = c.to_lowercase().collect();
        assert_eq!(lower.len(), 1, "U+{cp:X} lower-cases to {}", lower.len());
        if lower[0] != c {
            mapped.push((i64::from(cp), i64::from(lower[0] as u32) - i64::from(cp)));
        }
    }
    let mut out = String::new();
    let mut i = 0;
    while i < mapped.len() {
        let (first, delta) = mapped[i];
        let mut count = 1i64;
        let mut step = 1i64;
        if i + 1 < mapped.len() && mapped[i + 1].1 == delta && mapped[i + 1].0 - first <= 15 {
            step = mapped[i + 1].0 - first;
            count = 2;
            while i + (count as usize) < mapped.len() && count < 255 {
                let (c, d) = mapped[i + count as usize];
                if d == delta && c == first + count * step {
                    count += 1;
                } else {
                    break;
                }
            }
        }
        out.push_str(&format!("{first:05x}{count:02x}{step:01x}{:05x}", delta + 0x40000));
        i += count as usize;
    }
    out
}
