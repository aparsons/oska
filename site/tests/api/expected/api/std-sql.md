# std.sql

The SQL sink. See <b>this</b>.

> **Checks but does not run.** `oskac check` accepts a program that uses this, but `oskac run`, `oskac test` and `oskac build` refuse it. See [What works today](/guide/what-works-today.md).

`oskac doc std.sql` prints this page offline.

## connect

**Kind:** fn · **Authority:** NetConnect (net)

```oska
pub fn connect(net: NetConnect, url: String) -> Result<Db, SqlError>
```

> **Checks but does not run.** `oskac check` accepts a program that uses this, but `oskac run`, `oskac test` and `oskac build` refuse it. See [What works today](/guide/what-works-today.md).

Connect to the database at `url`.

## Db.\*query\*

**Kind:** method · **Authority:** <img src=x onerror=alert(1)> \_em\_ \`tick\`

~~~oska
```</code></pre><script>alert(1)</script>
~~~

> **Checks but does not run.** `oskac check` accepts a program that uses this, but `oskac run`, `oskac test` and `oskac build` refuse it. See [What works today](/guide/what-works-today.md).
