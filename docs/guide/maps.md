# Maps

A `Map<K, V>` finds a value by its key and keeps its entries in the order the keys were first put. Import it with `use std.map`; it is Oska code in the standard library, not part of the prelude. A map is a value, like a list: it is copied on write, and a method that changes it (`put`, `remove`) needs a `var` binding. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints. The words in **bold** are the project's [glossary](../../GLOSSARY.md) terms.

## Put, get, and order

`map.Map.new()` makes an empty map; give it its type in the binding. `put` sets the value of a key, and `get` returns an `Option` of the value: a missing key is `None`, never a panic. `entries()`, `keys()` and `values()` give the entries in **insertion order**, the order the keys were first put. They never depend on a hash, so every platform and both backends print the same order.

Two rules keep the order easy to predict:

- `put` on a key the map holds keeps the key's place and changes its value.
- `remove` takes the key out, and a later `put` of it puts it last.

```oska
use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    var stock: map.Map<String, Int> = map.Map.new()
    stock.put("pear", 3)
    stock.put("apple", 5)
    stock.put("plum", 8)

    stock.put("apple", 6)
    let removed = stock.remove("pear")
    stock.put("pear", 4)

    var line = ""
    for e in stock.entries() {
        let n = e.value
        line = "{line} {e.key}={n}"
    }
    io.println("order:{line}")?

    let plum = match stock.get("plum") {
        Some(n) => n
        None => 0
    }
    let fig = match stock.get("fig") {
        Some(n) => n
        None => 0
    }
    let count = stock.len()
    io.println("plum {plum}, fig {fig}, {count} entries")?
    Ok(())
}
```

```console cmd=run
order: apple=6 plum=8 pear=4
plum 8, fig 0, 3 entries
```

Things to notice:

- `apple` was put second and updated later, so it stays first. `pear` was removed and put again, so it moved to the end.
- `remove` gives the removed value as an `Option`, here ignored with `let removed`. Unlike a `Result`, an unused `Option` is not an error.
- Two maps are equal (`==`) when they hold the same entries in the same order.
- `len()` and `is_empty()` count the entries, and `contains_key(key)` asks without reading the value.

## Building a map from a list

`map.Map.of(entries)` puts a list of `map.Entry` values in order, as repeated `put` calls would: a key that occurs again keeps its first place and takes the later value.

```oska
use std.map

fn main(io: Stdout) -> Result<(), IoError> {
    let ports: map.Map<String, Int> = map.Map.of([
        map.Entry(key: "http", value: 80),
        map.Entry(key: "https", value: 443),
        map.Entry(key: "http", value: 8080),
    ])
    var line = ""
    for e in ports.entries() {
        let n = e.value
        line = "{line} {e.key}={n}"
    }
    io.println("ports:{line}")?
    Ok(())
}
```

```console cmd=run
ports: http=8080 https=443
```

There is no map literal such as `{"a": 1}` yet: braces already open a block, so a literal needs a language decision that is still open ([spec #591](https://github.com/aparsons/oska/issues/591)). Use `map.Map.of` until then.

## Keys

A key type needs `Eq` and `Hash`. These are keys: the integer types, `Bool`, `String`, `Path`, `Bytes`, and an `Option` or a `List` of a key type. A struct or an enum is a key when it has an `impl Eq` and every field is a key. A type that is not a key is `E0266` where a `Map` method is called, and a `Secret` is refused as a key (`E0351` and `E0354`) because a hash or a probe order would leak timing. A `Float` key will be refused once `Float` exists.

A key keeps its **trust** flag: the keys of a map of `Untrusted<String>` are still untrusted when you read them back. See [untrusted data](untrusted-data.md).

## Limits

The hash is fixed and has no seed, so a person who picks keys can try to make them collide. A map bounds that damage:

- A lookup looks at 64 slots at most. A `put` that cannot place its key that close to where it hashes rebuilds the table under the next of two fixed mixes of the hash, and when all three hashes fail it panics with `P0045`.
- A map holds at most 4,194,304 entries. `map.Map.with_max_entries(n)` sets a lower limit. A `put` of a new key past it panics with `P0044`; an update of a key the map holds is not a new entry.
- Each probe is a step under `--max-steps`, the same on both backends.

Both panics are listed in [debugging](debugging.md).

## JSON objects

`std.json`'s `value.as_map()` gives an object as a `map.Map<Untrusted<String>, Value>` in the order its members were written, so you can look a member up with `get`. See the changelog for its errors.
