---
name: crate-boundaries
description: Enforce Oska's crate graph as deep modules, with a checked list of allowed crate edges.
disable-model-invocation: true
---

# crate-boundaries

This skill makes the crate graph of a Cargo workspace a checked rule. A crate may depend only on the crates that an allowed-edges file names. A check script fails on a new edge, a missing crate, or a cycle. The skill also teaches the visibility side: what each crate shows to other crates.

The words "deep module", "interface", and "seam" have the meanings of the `codebase-design` skill. To use them in depth, call the Skill tool with `codebase-design`. In short: a crate is a module, its public API is its interface, and an allowed edge is a seam that another crate may use.

## Oska's graph

Oska's compiler has 7 crates, and no crate from outside the repo. These are the allowed edges:

| Crate | May depend on |
|---|---|
| `oska-syntax` | nothing |
| `oska-sys` | nothing |
| `oska-check` | `oska-syntax` |
| `oska-eval` | `oska-syntax` |
| `oska-native` | `oska-check`, `oska-syntax` |
| `oska-run` | `oska-check`, `oska-native`, `oska-syntax`, `oska-sys` |
| `oskac` | `oska-check`, `oska-native`, `oska-run`, `oska-syntax`, `oska-sys` |

`oska-sys` is the only crate with `unsafe` code (ADR 0009). The `scripts/check.sh` step "unsafe only in oska-sys" enforces that. An edge to `oska-sys` gives a crate access to system calls. So a new edge to `oska-sys` needs an explicit reason, and the user must accept it.

## The script

`scripts/check-graph.sh` in this skill folder reads each `crates/*/Cargo.toml`. It reads the package name and the path dependencies in `[dependencies]`, `[dev-dependencies]`, and `[build-dependencies]`, also in `[target.….dependencies]`. A dev-dependency is an edge too.

- `check-graph.sh --show` prints the current graph, one `crate: dep dep` line for each crate.
- `check-graph.sh` compares the graph with `scripts/crate-graph/allowed.txt`. That file has the same form.
- Exit 0 is a pass. Exit 1 is a new edge, a missing crate, or a cycle. Exit 2 is input that it cannot read.

The script fails closed. It refuses a dependency that it cannot parse, for example `workspace = true`, a multi-line inline table, or a registry version. Run `check-graph.sh --help` for the full usage. `scripts/test.sh` is its test suite.

## Steps

### 1. Show the current graph

1. Run `bash .claude/skills/crate-boundaries/scripts/check-graph.sh --show` from the repo root.
2. Show the output to the user as a table, like the one in "Oska's graph".
3. Compare it with "Oska's graph". Tell the user about each difference.

If the script exits with 2, show its message. Do not edit it to make it pass. Fix the manifest, or ask the user.

Done when: the user has seen the current graph and each difference.

### 2. Agree the allowed edges

1. Propose the edges from "Oska's graph" as the default.
2. Ask the user to accept or change them. For each edge to `oska-sys`, ask for the reason.
3. Write the agreed edges to `scripts/crate-graph/allowed.txt`, one line for each crate. Add a comment line with the reason above each edge to `oska-sys`.

If the user is not present, use "Oska's graph" and write down the assumption. Do not add an edge that the table does not have.

Done when: `scripts/crate-graph/allowed.txt` has a line for each of the 7 crates, and the user agreed to it.

### 3. Install the check

1. Copy `scripts/check-graph.sh` from this skill folder to `scripts/crate-graph/check.sh`.
2. Copy `scripts/test.sh` to `scripts/crate-graph/test.sh`. With no argument, it tests `check.sh` next to it.
3. Run `chmod +x` on both files.
4. Read the "Check steps" section of `docs/agents/implementer.md`. Add two step blocks to `scripts/check.sh`, near the other guard steps:

   ```bash
   if step "crate graph" --no-slot --fast-when 'crates/*/Cargo.toml' --fast-when 'scripts/crate-graph/*'; then
     bash scripts/crate-graph/check.sh
   fi

   if step "crate graph tests" --no-slot --fast-when 'scripts/crate-graph/*'; then
     bash scripts/crate-graph/test.sh
   fi
   ```

5. Run `bash scripts/crate-graph/test.sh`, `bash scripts/crate-graph/check.sh`, and `bash scripts/check-steps/test.sh`.
6. Run `shellcheck` and `bash -n` on both new scripts.

If a run fails, fix the cause. Do at most 3 rounds. Then stop and report to the user.

Done when: the test suite prints `all N cases passed`, the check passes, and `scripts/check.sh --list` shows both steps.

### 4. Prove that the check bites

Do this in a temporary copy. Do not change the real manifests.

1. Make the copy:

   ```bash
   tmp=$(mktemp -d)
   mkdir -p "$tmp/crates"
   for d in crates/*/; do mkdir -p "$tmp/$d" && cp "$d/Cargo.toml" "$tmp/$d"; done
   ```

2. Run `bash scripts/crate-graph/check.sh --repo "$tmp" --allowed scripts/crate-graph/allowed.txt`. It must pass.
3. Add a fake edge under the `[dependencies]` line. Do not append it at the end, because the file ends with `[lints]`.

   ```bash
   f="$tmp/crates/oska-syntax/Cargo.toml"
   sed -i '/^\[dependencies\]$/a oska-run = { path = "../oska-run" }' "$f"
   ```

4. Run the check again. It must exit 1 and name the new edge and a cycle.
5. Remove the fake line with `sed -i '/^oska-run = /d' "$f"`. Run the check again. It must pass.
6. Run `rm -r "$tmp"`.

Done when: you saw pass, fail, pass, in that order, and the temporary copy is gone.

### 5. Teach the visibility side

The graph controls which crates can see each other. Visibility controls what they see. Tell the user these rules, and check the crate that the user works on:

- **The public surface is `lib.rs`.** Each crate shows its interface through the `pub use` re-exports in its `lib.rs`. Keep the interface small, so that the crate stays a deep module.
- **Modules are `pub(crate)` by default.** Make an item `pub` only when another crate needs it. Then re-export it from `lib.rs`.
- **Tests use the public API.** Put integration tests under `crates/<crate>/tests/`. They see only what other crates see. For the test seams, see the `tdd` skill.
- **A new edge is a design decision.** Before you add one, ask if the code belongs in the lower crate. Change `allowed.txt` in the same commit as the manifest, with the reason in the commit message.

Done when: the user knows the four rules, and you listed any `pub` item in the crate that `lib.rs` does not re-export.

## Other workspaces

The script works on any Cargo workspace with its crates under `crates/`. In step 2, use the output of `--show` as the proposal, and agree each edge with the user. Skip the `oska-sys` rules.

## Security

- **S5 Git safety.** Do not weaken the check, its tests, or `allowed.txt` to make a build pass. A new edge needs the user's yes.
- **S9 Scripts fail closed.** The script uses `set -euo pipefail`. It refuses input that it cannot read. Keep this when you edit the installed copy.
- **Edges to `oska-sys`.** A new edge to `oska-sys` widens access to system calls. Write the reason in `allowed.txt`, and get the user's yes first.
- **S3 Least privilege.** Do step 4 in a temporary copy. Do not change the real manifests to test the check.
- **S8 Files.** Make temporary directories with `mktemp -d`, and remove them when you are done.
