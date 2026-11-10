# Untrusted data and secrets

Most injection bugs come from one mistake: text that arrived from outside the program ends up deciding what code, command, query, or path runs. Oska tracks that in the type system, so the mistake is a compile error with a stable code instead of a review comment. This page covers untrusted values, validators, sinks, and secrets.

## Trust is a flag on text

Text from outside the program is an **untrusted value**, written `Untrusted<String>` in a signature. Plain `String` means trusted. Despite the spelling, `Untrusted<String>` is a flag the checker tracks and not a wrapper type ([ADR 0006](../adr/0006-trust-is-a-flag-on-text.md)). At runtime it is an ordinary string.

Where untrusted text comes from:

- Anything read through a capability: `FsRead.read_to_string`, `Stdin.read_line`, `Env.get`.
- **Program inputs**: `Args.get`.
- The `message` of an `IoError`, `SqlError`, or `ProcError`.

The flow rule is one-way. Trusted text may go anywhere untrusted text is accepted. Untrusted text may never go where trusted `String` is required, and the checker reports `E0300`:

```oska error=E0300
fn greet(name: String) -> String {
    "hello"
}

fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    let who = args.get(1).unwrap_or("friend")
    let message = greet(who)
    io.println(message)
}
```

Text operations keep the flag. If any text input is untrusted, every text in the result is untrusted. Results that are not text are trusted, so inspecting untrusted text is free:

- `untrusted.trim()`, `untrusted.lines()`, and `untrusted.split(",")` are untrusted.
- `untrusted.len()`, `untrusted.contains("@")`, and `untrusted.parse_int()` give a trusted `Int`, `Bool`, or `Option<Int>`.
- Interpolating an untrusted value makes the whole string untrusted.

In your own code, a `String` parameter, field, or return type is trusted. Code that accepts raw input declares `Untrusted<String>`. User functions cannot be generic over trust.

## Validators and trust assertions

A **validator** inspects an untrusted value and, only if it passes its checks, returns a trusted value of a validated type. The vouching step is the **trust assertion** `.assume_trusted()`, and it normally lives inside the validator.

```oska
struct Email { raw: String }

enum EmailError {
    Missing,
}

impl Email {
    fn parse(s: Untrusted<String>) -> Result<Email, EmailError> {
        if !s.contains("@") {
            return Err(EmailError.Missing)
        }
        Ok(Email(raw: s.assume_trusted()))
    }
}

fn describe(email: Email) -> String {
    "mail to {email.raw}"
}
```

Downstream code takes an `Email`, so it never sees raw text. Shadowing keeps the result tidy: `let email = Email.parse(email)?` replaces the raw value with the validated one.

## Audit sites

Every `.assume_trusted()` and every secret `.expose()` is an **audit site**: a place where code deliberately weakens a guarantee. `oskac check` lists them all, with the byte span of the call, so a reviewer can find every one without reading the code.

```oska
fn vouch(name: Untrusted<String>) -> String {
    name.assume_trusted()
}
```

```console cmd=check
fn vouch: pure
main.osk: audit: assume_trusted (bytes 50..71)
```

With `--format=json` they are in the `audit` array as `{"kind":"assume_trusted","span":{...}}` or `{"kind":"expose",...}`. A change that adds an audit site is the part of a diff most worth reading.

## Sinks

A **sink** is an API where data leaves the program or changes how it behaves: SQL, processes, HTML, paths, network hosts, logs. Each sink separates two kinds of argument:

- A **data position** accepts untrusted values, because the sink treats it only as data. Examples are SQL parameters, process arguments, and escaped HTML text.
- A **structural position** rejects untrusted values, because it decides code, commands, or destinations. Examples are SQL query text, a program name, `TrustedHtml`, a restricted directory, and a network host. A structural position is any parameter declared `String`.

| Sink | Data position | Structural position |
| --- | --- | --- |
| `std.sql` | `Db.query` parameters | The query text, which must be an `sql"..."` literal, and `connect`'s `url` |
| `std.proc` | `run`'s arguments | The program name |
| `std.html` | `text(...)`, which is escaped | `TrustedHtml.new` |
| Paths | `Path.from` | The directory given to `restrict_to` |
| Network | `Connection.send`'s body | `NetConnect.connect`'s host |
| Output | `Stdout.println`, `Stderr.println` | |

A **sink literal** such as `sql"SELECT ..."` provides text for a structural position. It can never interpolate, so there is no way to build a query from pieces. The tag must name an imported module, so `sql"..."` needs `use std.sql`.

```oska
use std.sql

fn user_name(db: sql.Db, id: Untrusted<String>) -> Result<Int, sql.SqlError> {
    let rows = db.query(sql"SELECT name FROM users WHERE id = ?", [id])?
    Ok(rows.len())
}
```

The user's `id` reaches the query only as a parameter. Passing any text where the query literal belongs is `E0341`:

```oska error=E0341
use std.sql

fn user_name(db: sql.Db, id: Untrusted<String>) -> Result<Int, sql.SqlError> {
    let rows = db.query("SELECT name FROM users WHERE id = ?", [id])?
    Ok(rows.len())
}
```

A sink literal whose module is not imported is `E0310`, an unknown tag is `E0311`, and a module export written without its qualifier is `E0340`. `std.html` runs in this release. `std.sql` and `std.proc` check but do not run yet: `oskac run` refuses a program that imports one with `E0371`.

`std.html` escapes untrusted text in a data position. `text(...)` escapes `&`, `<`, `>`, `"`, and `'`, while `TrustedHtml.new` takes trusted markup, so putting untrusted text into markup unescaped needs an explicit, audited `.assume_trusted()`:

```oska
use std.html

fn page(name: Untrusted<String>) -> html.Html {
    let open = html.Html.raw(html.TrustedHtml.new("<p>"))
    let close = html.Html.raw(html.TrustedHtml.new("</p>"))
    open.concat(html.text(name)).concat(close)
}

fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    let name = args.get(1).unwrap_or("friend")
    io.println(page(name).render())
}
```

```console cmd=run -- <b>hi</b>
<p>&lt;b&gt;hi&lt;/b&gt;</p>
```

## Paths

`Path.from` accepts untrusted text, because the protection for paths comes from attenuation and not from trust tracking. Attenuate to a directory capability before you read a path a user chose:

```oska
fn read_user_file(fs: DirRead, requested: Untrusted<String>) -> Result<Int, IoError> {
    let text = fs.read_to_string(Path.from(requested))?
    Ok(text.len())
}

fn main(io: Stdout, fs: Fs, args: Args) -> Result<(), IoError> {
    let public = fs.restrict_to("public").read_only()
    let name = args.get(1).unwrap_or("index.txt")
    let size = read_user_file(public, name)?
    io.println("{size} characters")?
    Ok(())
}
```

The runtime rejects any path that escapes the restricted directory. See [capabilities](capabilities.md#attenuation).

## Secrets

`env.secret("API_KEY")` returns a `Secret<String>`. In contrast, `env.get` returns an `Untrusted<String>`. A **secret value** is never displayed, logged, or interpolated:

- It has no `Display`, so it cannot be printed or turned into text (`E0352`).
- It cannot be interpolated, nor can any value that holds one, such as a `List<Secret<String>>` or a struct with a `Secret` field (`E0350`).
- It cannot be compared with `==` or `!=`, because that would leak timing information (`E0351`). Use `constant_time_eq(text)`, or `constant_time_eq_secret(other)` for two secrets.
- `dbg` prints it as `<redacted>`.
- The only way to read it is `.expose()`, which returns trusted text and is an audit site.

```oska
fn check_key(key: Secret<String>, offered: Untrusted<String>) -> Bool {
    key.constant_time_eq(offered)
}

fn main(io: Stdout, env: Env, args: Args) -> Result<(), IoError> {
    let key = env.secret("SERVICE_KEY")
    let offered = args.get(1).unwrap_or("")
    if check_key(key, offered) {
        io.println("welcome")?
    }
    Ok(())
}
```

If the variable is not set, `env.secret` panics with `P0020` instead of continuing with an empty stand-in, so an empty password can never slip through.

Each mistake has a stable code:

```oska error=E0350
fn leak(key: Secret<String>) -> String {
    "Bearer {key}"
}
```

```oska error=E0351
fn same(key: Secret<String>, offered: String) -> Bool {
    key == offered
}
```

The fix attached to `E0351` rewrites `a == b` to `a.constant_time_eq(b)`, or to `a.constant_time_eq_secret(b)` when both sides are secrets.
