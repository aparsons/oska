# What works today

Oska is pre-alpha, and not everything that checks runs yet. This page lists each feature and says whether `oskac check` accepts it, whether it runs, and where. Read it before you write code that has to run: a program can pass `oskac check` and still be refused by `oskac run`.

The columns:

- **Checks:** `oskac check` accepts a program that uses the feature.
- **Interpreter:** `oskac run --interpret` runs it. The interpreter runs on every host: Linux, macOS and Windows.
- **Native:** `oskac build` compiles it to an executable for Linux or macOS, on aarch64 or x86_64 (`--target`), from any host. On Linux and macOS, `oskac run` and `oskac test` compile natively too; on Windows they use the interpreter. See [platform support](cli-reference.md#platform-support).
- **Platforms:** where a program that uses the feature runs today.

A **No** names the code `oskac` refuses the program with, before any of it runs. `E0370` and `E0371` mean the feature has no runtime yet, and `E0390` that the native backend cannot compile it yet. `oskac explain <code>` says more.

| Feature | Checks | Interpreter | Native | Platforms |
| --- | --- | --- | --- | --- |
| Core language: structs, enums, `match`, generics, closures, lists | Yes | Yes | Yes | Every host; native on Linux and macOS |
| Traits and `impl` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| Contracts: `requires` and `ensures` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| Packages: `oska.toml` and `use pkg.…` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `test` blocks and `oskac test` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `Stdout`, `Stderr` and `Args` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `Env` and `Secret` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `Clock` and `Random` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| Files: `Fs`, `FsRead`, `FsWrite`, and the directory and file capabilities | Yes | Yes | Yes | Every host; native on Linux and macOS |
| Structured concurrency: `scope`, `spawn` and `join` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `Untrusted` text and `assume_trusted` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `std.html` | Yes | Yes | Yes | Every host; native on Linux and macOS |
| `std.sql` | Yes | No (`E0371`) | No (`E0371`) | None yet: checks only |
| `std.proc` and `Spawn` | Yes | No (`E0371`) | No (`E0371`) | None yet: checks only |
| `Net`, `NetConnect` and `NetListen` | Yes | No (`E0370`) | No (`E0390`) | None yet: checks only |

`std.sql`, `std.proc` and `Net` check but do not run. Their signatures are complete, so `oskac check` enforces their rules today: untrusted text cannot reach a query, a command or a host. A program that uses them is refused by `oskac run`, `oskac test` and `oskac build`. Plan around them: keep that code behind a function you can test without it, or wait for a release that runs it.

Within a feature that runs, a few constructs do not yet: the interpreter refuses them with `E0380`, and the native backend refuses them with `E0390`, an error on every platform, which `--interpret` works around. The [runtime notes](../runtime.md#native-builds) list what the native backend compiles, and [troubleshooting](troubleshooting.md) lists the known gaps.

`cargo test` holds this table to the toolchain: a small program for each row must check, run and build exactly as the row says.
