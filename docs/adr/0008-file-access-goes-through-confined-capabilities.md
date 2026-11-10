# File access goes through confined capabilities, and `Fs` only attenuates

## Decision

`Fs`, `FsRead` and `FsWrite` grant no file access themselves. Their only methods are attenuations: `restrict_to`, `restrict_to_file`, `read_only` and `write_only`. Every read and write goes through a confined capability:

- **Directory capabilities** confine access to one directory: `Dir`, `DirRead` and `DirWrite`, which provide `read_to_string(Path)`, `list_dir(Path)`, `write(Path, text)` and `create_dir(Path)`.
- **File capabilities** confine access to one file: `FileRead` and `FileWrite`, which provide `read_to_string()` and `write(text)`, with no path.

Code that really needs the whole filesystem must write `restrict_to("/")`, and the authority report shows that root.

The first root, cut from `Fs`, `FsRead` or `FsWrite`, is a trusted `String`, so it is a structural position. Narrowing an existing directory capability further, with `restrict_to` or `restrict_to_file`, takes a `Path` that may come from untrusted text, because the result stays confined under the same root. `Path` carries no trust.

## Why

Before this decision, a restricted and an unrestricted `FsRead` were the same type. The checker therefore could not see code reading a user-chosen path through an unrestricted capability, which is path traversal across everything the process can read (#48).

Making confined access the only kind of file access closes that hole with one rule. A signature such as `fn build(docs: DirRead, out: DirWrite, readme: FileRead)` states exactly where the function can read and write.

## Considered options

- **Separate restricted types, with trust tracked on `Path`.** `restrict_to` would return a new type, and only that type would accept paths built from untrusted text. Unrestricted `FsRead` would keep reading any trusted path. This was the first decision on #48. It was rejected because it needs trust tracked on `Path`, as a flag or as two path types, on top of the trust flag on text ([ADR 0006](0006-trust-is-a-flag-on-text.md)). It also leaves two ways to read a file, which means one more rule for agents to get wrong.
- **Keep one type and rely on runtime confinement.** Rejected: that is the hole #48 describes.

## Consequences

- **Programs that read anywhere say so.** They write `restrict_to("/")`, which is visible in the code and in the authority report.
- **Package authority has an order to compare against.** It is counted in built-in capability kinds ordered by attenuation: `Fs` > `FsRead` > `DirRead` > `FileRead`, and likewise for writes. Moving up that order is a **widening**.
- **This is a breaking change to the prelude, with no deprecation period.** The guides, the examples and the site generator move to the new API in the same change.
