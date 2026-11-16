# The install receipt

The installers write `receipt.json` into the Oska home (`~/.oska`, or `OSKA_HOME`). It records what they installed, so that an uninstall removes that and nothing else. Three programs uninstall from it: `install.sh --uninstall`, `install.ps1 --uninstall` and [`oskac self uninstall`](cli-reference.md#oskac-self). This page is the one definition of the receipt and of what each of them does with every entry. The [install guide](install.md#the-install-receipt) shows a receipt and what else it records.

The rules are checked by shared fixtures, in `scripts/install/receipt-fixtures` in the source: `scripts/install/test.sh`, `scripts/install/test.ps1` and `oskac`'s `receipt_fixtures` tests all run them. A rule changed here changes the fixtures, and every implementation must then pass them.

## Schema 1

The receipt is one JSON object. Uninstall reads these fields; any other field is kept and ignored.

- `schema`: the JSON integer `1`. Anything else, `1.0` included, is another schema.
- `version`: the version installed, such as `"0.4.0"`.
- `files`: an array with one object per thing the installer wrote. Each has a `path` and a `kind`, both strings. The other fields depend on the kind: `sha256` (lowercase hex SHA-256 of the file as written), `line` (a profile line) and `project` (a `--hook` project directory).

A receipt is refused as a whole (`unsafe_path`, nothing removed) when uninstall cannot read it for certain:

- it is not exactly one JSON value, or an object repeats a key (`install.ps1` also counts keys that differ only in case as repeated, as PowerShell cannot tell them apart);
- it is not schema 1, or has no `files` array, or an entry is not an object with a string `path` and `kind`;
- a value in a `files` entry holds a control character;
- a `\u` escape anywhere in it stands for anything but printable ASCII (U+0020 to U+007E). A character outside ASCII written as itself is fine.

## Entry kinds

Every entry is checked before anything is removed. An entry whose kind is not one of these, or which is not in the place its kind may be, refuses the whole uninstall with `unsafe_path`.

| Kind | Written by | Where it may be | `sha256` |
| --- | --- | --- | --- |
| `binary` | both installers | directly in the Oska home or its `bin` directory | always |
| `record` | both installers | directly in the Oska home or its `bin` directory | except the receipt's own entry |
| `profile_line` | `install.sh --add-to-path` | `.zshrc`, `.bashrc`, `.bash_profile` or `.profile` directly in `HOME`, with a `line` that starts `export PATH=` and ends `# added by the Oska installer` | never |
| `user_path_entry` | `install.ps1 --add-to-path` | its `path` is the `bin` directory (any case, trailing separator aside) | never |
| `hook_script` | `install.sh --hook` | `<project>/.claude/hooks/oska-check.sh` | always |
| `hook_settings` | `install.sh --hook` | `<project>/.claude/settings.json` | only when the installer created the file |

For every kind:

- `path` (other than a `user_path_entry`'s) is absolute, with no `.` or `..` part.
- No path is in a `.config/oska` directory (settings and credentials, which uninstall never touches), and no path is reached through a symlink: not the entry, not the directory it is in, not the Oska home or any directory above it. On Linux and macOS a symlink owned by root, such as macOS's `/var`, is allowed, as only an administrator can make one.
- A hook entry's `project` is absolute, with no `.` or `..` part.
- A kind is accepted only on the platform whose installer writes it: on Windows a `profile_line` or hook entry is refused, and on Linux and macOS a `user_path_entry`. `install.ps1 --hook` is not available yet (issue #87).

## What uninstall does with each entry

Each entry gets one outcome: **remove** it, **keep if changed** (remove it only if it is shown to be what the installer wrote, else leave it and list it in `kept`), **leave to the installer** (edit nothing, say what to do in `path_hint`), or **refuse** (the whole uninstall, before anything is removed).

| Kind | `install.sh --uninstall` | `install.ps1 --uninstall` | `oskac self uninstall` |
| --- | --- | --- | --- |
| `binary` | keep if changed: its SHA-256 | keep if changed: its SHA-256 | keep if changed: it must be the running `oskac`, and the receipt's `version` its version |
| `record`, the receipt | remove, last | remove, last | remove, last, if this `oskac` is the install the receipt records |
| `record`, the `version` file | keep if changed: its SHA-256 | keep if changed: its SHA-256 | keep if changed: it must hold the receipt's `version` and a newline |
| `record`, any other | keep if changed: its SHA-256 | keep if changed: its SHA-256 | keep: `oskac` computes no SHA-256 to show it unchanged |
| `profile_line` | remove the line, if it is still there exactly as written for this `bin` directory; else keep the file and say in `path_hint` | refuse | leave to the installer |
| `user_path_entry` | refuse | remove the entry from the user `PATH` if it is there; else list the `bin` directory in `kept` and say in `path_hint` | leave to the installer |
| `hook_script`, `hook_settings` | keep if changed: its SHA-256 | refuse | keep if changed: byte for byte the published file this `oskac` carries |
| anything else | refuse | refuse | refuse |

Also, for every implementation:

- An entry with no `sha256`, other than the receipt's own entry, is kept: nothing shows it unchanged. `hook_settings` without one are settings the installer found rather than created, kept with a hint to remove the `oska-check.sh` hook from them by hand.
- When the receipt does not record the running `oskac` (another version, or another copy), `oskac self uninstall` keeps every `binary` and `record` entry and goes on with the rest. The installers do not run `oskac`, and go by SHA-256 alone.
- A path listed twice is checked each time and acted on, and reported, once. A listed path that does not exist is skipped; one that is a directory is kept, except `bin`, which is handled with the Oska home, at the end.
- The receipt is removed last, and kept if any file could not be removed, so that a second run can finish.
- Then `bin` and the Oska home are removed if they are left empty, and listed in `kept` if not. Nothing is removed recursively.
- On Windows, `oskac self uninstall` cannot delete the running `oskac.exe`: it moves it into the temporary directory and lists it under `pending`.

## The fixtures

`scripts/install/receipt-fixtures` has one directory per case, with two files:

- `receipt.json`: the receipt, as text, so that a case can spell it in a way a JSON library would not write back (`1.0`, a `\u` escape).
- `case.json`: what the case is about (`about`), what to put in place before the uninstall (`setup`, then `change`), and what each implementation must do (`expect`).

Each harness makes a scratch user home and puts, before the setup, a `binary` at `@EXE@` (a copy of `oskac` for `oskac`, a stand-in for the installers) and the `version` file holding `@VERSION@` and a newline. It writes the receipt to `@OSKA_HOME@/receipt.json`, then applies `change`, then runs the uninstall with `OSKA_HOME` set to `@OSKA_HOME@` and the user's home to `@HOME@` (`test.ps1` leaves `USERPROFILE` as it is, as PowerShell writes its own files there).

Placeholders, in `receipt.json` and in the paths and texts of `case.json`:

| Placeholder | Stands for |
| --- | --- |
| `@HOME@` | the user's home |
| `@OSKA_HOME@` | `@HOME@/.oska` |
| `@BIN@` | `@OSKA_HOME@/bin` |
| `@EXE@` | `@BIN@/oskac`, or `@BIN@/oskac.exe` on Windows |
| `@PROJECT@` | `@HOME@/project` |
| `@VERSION@` | the version the harness installs (the running `oskac`'s, for `oskac`) |
| `@SHA:name@` | the SHA-256 of the setup file called `name` as set up (`exe` and `version` are the two put in place first); `oskac`'s harness writes a fixed digest, as it checks none |

In a path, `/` stands for the platform's separator: on Windows each harness writes `\` instead, in the receipt too. Each `setup` and `change` entry has a `path` and one of `text` (the file's contents), `published` (`hook_script` or `hook_settings`, the file from `site/agent-setup`) or, in `setup` only, `link_to` (a symlink, or on Windows a junction or symlink, to that path). A `setup` entry may have a `name`, for `@SHA:name@`.

`expect` has an outcome for each of `sh`, `ps1`, `oskac_unix` and `oskac_windows`:

- `{"refused": "unsafe_path"}`: the uninstall exits 1 with that error, and no file under the scratch home changed.
- Otherwise it exits 0, and `removed` and `kept` are the lists it prints, in any order (for `oskac` on Windows, `pending` counts as removed). `path_hint` says whether its `path_hint` is a string, and `contents`, if there, maps a path to the text that file must hold afterwards. Every path it lists as removed is gone, and every file it does not list is still there.

To add a kind, add it to the tables above and add fixtures that use it, with an outcome for every implementation. The tests fail until the kinds here, the kinds the fixtures use and the kinds each implementation writes and reads agree.
