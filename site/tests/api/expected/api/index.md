# API reference

Every public item of the prelude, which every module sees, and of the `std` modules a program can `use`: its signature, its authority, whether it runs, and what it does. It is generated from the sources the checker reads, so it says what the checker accepts. `oskac doc` prints the same reference offline.

All of it is in [api.json](https://oskalang.dev/api.json) as data.

- [prelude](https://oskalang.dev/api/prelude.md): The prelude: what every module sees without `use`.
- [std.sql](https://oskalang.dev/api/std-sql.md): The SQL sink. See <b>this</b>.
