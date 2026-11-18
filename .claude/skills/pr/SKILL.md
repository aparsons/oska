---
name: pr
description: Shape of a pull request body, with Summary, Evidence, and Merge risk sections. Use when you write or update a pull request description.
---

# Pull request body

A pull request body helps a reviewer understand the change fast. It shows what changed, proves that it works, and says how risky the merge is.

The steps below fit aparsons/oska, which has no pull request template. The repo is Oska when `crates/oskac/` and `scripts/check.sh` exist. In another repo, look first for `.github/pull_request_template.md`, `.github/PULL_REQUEST_TEMPLATE.md`, or `.github/PULL_REQUEST_TEMPLATE/`. If one exists, fill it and put the sections below inside it.

## Steps

1. **Write the issue line.** Start the body with the issue links:
   - a ticket: `Closes #<ticket>. Part of spec #<spec>.`
   - a spec branch: `Closes #<spec>.` and one line per ticket;
   - review fixes: `Closes #<follow-up issue>.` and the pull request that the review came from.
   Done when: the first line names each issue that the pull request closes or belongs to.

2. **Write the Summary.** Write one to three short sentences. Use terms from `GLOSSARY.md`, not the words that it lists under _Avoid_. Add the smallest visual that makes the change clear. Pick one or a few visual types from [VISUALS.md](VISUALS.md), not all. A `diff` block of a changed type or signature works well for compiler changes. Put each visual next to the sentence that it supports.
   Done when: a reviewer can say what changed from the Summary alone.

3. **Write the Evidence.** Show the state before and after the change. Use the best evidence that you have:
   1. The test that failed before the change and passes after it, by file and name, for example `crates/oska-run/tests/dir_list.rs::lists_only_inside_the_root`.
   2. A golden file diff under `crates/oskac/tests/golden/`, when CLI output changed on purpose.
   3. The `scripts/check.sh` result: full or `--fast`, and each step that skipped, with its reason, such as "PowerShell step skipped: pwsh not installed".
   4. The CI result per platform: Linux, macOS, and Windows.
   5. Screenshots, only for a change to the site.

   Remove secrets and private data from the evidence first.
   Done when: the Evidence names a test or a golden diff, and states the check tier and each skipped step.

4. **Write the Merge risk.** Write "one-way door" or "two-way door". A two-way door is a merge that you can undo cheaply with a revert. A one-way door is hard to undo. In Oska, these are one-way doors:
   - a change to release workflows, `scripts/release/`, the installers, or the signing key (follow `docs/agents/release.md`);
   - a published format, such as diagnostic JSON, `oska.toml`, or an error code that users match on;
   - a new ADR, or a change that contradicts one in `docs/adr/`.

   Then write the blast radius as one word: `none`, `docs`, `crate`, `workspace`, `site`, `users`, or `release`.
   Done when: the section has a door type and a one-word blast radius.

5. **Add a Security line if needed.** Add it when the change touches capabilities, attenuation, trust on text, path confinement, input parsing, secrets, workflows, or the release path. Name the attack test that covers it.
   Done when: the line is present, or the change touches none of these areas.

6. **Mark the author.** End the body with the attribution lines that the session gives. They mark the body as written by an AI agent.
   Done when: the body ends with the attribution lines.

## Template

```markdown
Closes #<ticket>. Part of spec #<spec>.

## Summary

<one to three sentences, each with its visual next to it>

## Evidence

- Before: <test name> fails: <one line of output>
- After: <test name> passes. Golden diff: <file, or "none">.
- `scripts/check.sh` (full) passed. Skipped: <step: reason, or "none">.
- CI: Linux <result>, macOS <result>, Windows <result>.

## Merge risk

Two-way door. Blast radius: crate. <optional note>

Security: <what you checked and the attack test, only when needed>
```

Keep the prose short. A reviewer reads the visual first.

## Security

- **Redact evidence.** Remove secrets, tokens, internal URLs, and personal data from logs and screenshots. Write `<REDACTED>` in their place.
- **Security line.** Add the Security line for the areas in step 5.
- **Release paths are one-way doors.** Do not call a change to the release path or the signing key a two-way door.
- **Disclosure.** Mark the pull request body as written by an AI agent.
