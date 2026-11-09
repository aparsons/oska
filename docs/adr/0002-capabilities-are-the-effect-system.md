# Capabilities are the effect system

Oska has no effect annotations (`! {Net}`). A function's effects are exactly the capabilities it can reach through its capability-bearing parameters. These include:
- explicitly marked `cap` types
- `cap fn` closures

Nondeterminism also comes from capabilities (`Clock`, `Random`). That means a function with no capability-bearing parameters is pure and deterministic.

We chose one mechanism instead of two. Finding 5 of the research recommends declaring effects in signatures, and capability parameters already do that. A separate effect system would duplicate the same information, and the two could drift apart.

There are two deliberate exceptions:
- `dbg(...)`, which is a compile error in release builds
- panics, which stop the task but have no effect on the outside world and cannot be caught

## Capabilities in type arguments

Whether a type carries a capability is decided from the type as fully written out. `List<Stdout>` and `Box<Stdout>` are capability-bearing because `Stdout` appears in them. Neither `List` nor a user-defined `struct Box<T>` needs a `cap` declaration. `cap` is required only when a type's own fields name a capability directly.

The capability is still visible on the page, in the type the signature spells out. Requiring `cap` on generic containers would have been wrong for `List<Int>`, `Option<Int>`, and every other container of plain data.
