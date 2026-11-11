# Language tour

A fast tour of Oska, one idea at a time. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints. The full rules are in the [design notes](../design.md). The words in **bold** are the project's [glossary](../../GLOSSARY.md) terms.

## Functions, bindings, and blocks

A function states the type of every parameter and of its result. Nothing is inferred across a function boundary. The last expression of a block is its value, and `return` leaves early.

Bindings are immutable. `let` binds a value, and `var` makes a binding that can be assigned to. There is no `;`: a line break ends a statement.

```oska
fn add(a: Int, b: Int) -> Int {
    a + b
}

fn clamp_to_ten(n: Int) -> Int {
    if n > 10 {
        return 10
    }
    n
}

fn main(io: Stdout) -> Result<(), IoError> {
    let sum = add(3, 4)
    var best = clamp_to_ten(sum)
    best = best + 1
    io.println("best is {best}")?
    Ok(())
}
```

```console cmd=run
best is 8
```

Things to notice:

- `main(io: Stdout)` asks for the ability to write to standard output. This is the program's [capability](capabilities.md), and it is the only effect the program can have.
- `io.println(...)` returns a `Result`, and ignoring a `Result` is an error (`E0240`). `?` returns early from `main` if the print failed.
- `"best is {best}"` interpolates. A `{...}` hole holds only a name or a field path such as `{user.name}`. Compute anything else with `let` first.
- Arguments can be labeled, as in `add(a: 3, b: 4)`. Labels are optional, and the checker verifies them against the parameter names.
- `Int` is a 64-bit signed integer. Overflow is a **panic**, never a silent wrap.

## Structs, methods, and mutation

A struct is built with call syntax. Fields declared by name must all be named, in declaration order.

A method is declared in an `impl` block with `self` as its first parameter. A **mutating method** declares `var self`. It can be called only on a `var` binding, and no other parameter can be mutated.

```oska
struct Counter { count: Int, step: Int }

impl Counter {
    fn current(self) -> Int {
        self.count
    }

    fn tick(var self) {
        self.count = self.count + self.step
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    var counter = Counter(count: 0, step: 5)
    counter.tick()
    counter.tick()
    let now = counter.current()
    io.println("count is {now}")?
    Ok(())
}
```

```console cmd=run
count is 10
```

Values are copied when they are shared and then changed, so assigning a `var` never changes another binding. There are no references to mutate through and no shared mutable state.

## Enums and `match`

An enum's variants can carry data. Variants are declared with positional fields, `Rect(Int, Int)`, or named fields, `Circle { radius: Int }`. They are built and matched with call syntax.

`match` must be **exhaustive**. Each arm sits on its own line, with no commas. An arm after a catch-all arm is an error because it is unreachable.

```oska
enum Shape {
    Circle { radius: Int },
    Rect(Int, Int),
    Empty,
}

fn area(shape: Shape) -> Int {
    match shape {
        Shape.Circle(radius: r) => 3 * r * r
        Shape.Rect(w, h) => w * h
        Shape.Empty => 0
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let a = area(Shape.Rect(3, 4))
    let b = area(Shape.Circle(radius: 2))
    io.println("{a} {b}")?
    Ok(())
}
```

```console cmd=run
12 12
```

Capitalization is enforced: types, variants, and traits start with an uppercase letter, and everything else starts with a lowercase letter. In a pattern, a bare uppercase name is always a fieldless variant, so a misspelled variant is an error instead of a silent catch-all (`E0250`).

## Option, Result, and `?`

There is no null. An absent value is an `Option<T>`, and a failure is a `Result<T, E>`. Neither can be ignored. `?` returns the `None` or the `Err` early, from a function that returns the same kind.

```oska
fn first_number(text: String) -> Option<Int> {
    let parts = text.split(",")
    let first = parts.get(0)?
    first.parse_int()
}

fn describe(text: String) -> String {
    match first_number(text) {
        Some(n) => "found {n}"
        None => "nothing"
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(describe("42,7"))?
    io.println(describe("x,7"))?
    Ok(())
}
```

```console cmd=run
found 42
nothing
```

To cross from one kind to the other, convert explicitly, for example with `.ok_or(err)?`. A `?` on a `Result` whose error type differs from the function's needs an `impl From<E1> for E2`; the [capabilities example](examples/capabilities.md) has one.

## Loops, ranges, lists, and closures

`for x in xs` walks a `List` or a range. `a..b` excludes `b`. There is no `loop` keyword; write `while true`. `break` and `continue` work as usual.

```oska
fn sum_to(n: Int) -> Int {
    var total = 0
    for i in 0..n {
        total = total + i
    }
    total
}

fn main(io: Stdout) -> Result<(), IoError> {
    var numbers: List<Int> = List.new()
    numbers.push(1)
    numbers.push(2)
    numbers.push(3)
    let doubled = numbers.map(|x| x * 2)
    let first = doubled.get(0).unwrap_or(0)
    let count = doubled.len()
    let total = sum_to(5)
    io.println("{first} {count} {total}")?
    Ok(())
}
```

```console cmd=run
2 3 10
```

`List.get` returns an `Option`. `List.at` panics with `P0002` when the index is out of bounds, because that is a bug. A closure is written `|x| expr` or `|x| { ... }`. A closure that captures a capability has the type `cap fn`; see [capabilities](capabilities.md).

## Generics and traits

Traits are explicitly implemented interfaces. They can provide default methods. There are no trait objects, so all dispatch is static, and there is no inheritance or overloading. Bounds are written inline.

```oska
trait Describe {
    fn describe(self) -> String

    fn shout(self) -> String {
        "LOUD"
    }
}

struct Dog { name: String }

impl Describe for Dog {
    fn describe(self) -> String {
        "a dog"
    }
}

fn pick<T>(a: T, b: T, first: Bool) -> T {
    if first { a } else { b }
}

fn same<T: Eq>(a: T, b: T) -> Bool {
    a == b
}

fn main(io: Stdout) -> Result<(), IoError> {
    let rex = Dog(name: "Rex")
    io.println(rex.describe())?
    io.println(rex.shout())?
    let chosen = pick(1, 2, false)
    io.println("chosen {chosen}")?
    if same(chosen, 2) {
        io.println("it is two")?
    }
    Ok(())
}
```

```console cmd=run
a dog
LOUD
chosen 2
it is two
```

A generic function is checked once, against its bounds. `fn same<T: Eq>` can use `==` but not `+`.

Interpolating a value needs `Display`, which you implement for your own types. This block is a fragment, not a complete program, because `Dog` is declared elsewhere:

```oska fragment
impl Display for Dog {
    fn to_string(self) -> String {
        let n = self.name
        "dog {n}"
    }
}
```

Complete, with the declaration beside it:

```oska
struct Dog { name: String }

impl Display for Dog {
    fn to_string(self) -> String {
        let n = self.name
        "dog {n}"
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let rex = Dog(name: "Rex")
    io.println("hello {rex}")?
    Ok(())
}
```

```console cmd=run
hello dog Rex
```

The `let` is needed because a `{...}` hole that starts with `self` is not resolved in this release (`E0221`): copy the field into a local first. `Bool` has no `Display`, so a `Bool` cannot be interpolated (`E0265`).

## Text

Strings are UTF-8. `len()` counts characters (Unicode code points), and `byte_len()` counts bytes. The prelude's text methods are `lines`, `len`, `byte_len`, `trim`, `split`, `contains`, `starts_with`, `ends_with`, `replace`, `join`, `chars`, `slice`, `repeat`, and `parse_int`. `+` works only on numbers, so build strings by interpolation.

Text also carries a trust flag: text from outside the program is an **untrusted value**. That is why a plain `String` parameter is a **structural position**. See [untrusted data and secrets](untrusted-data.md).

## Contracts

`requires` and `ensures` clauses sit between a function's return type and its body, one per line. `ensures |r| ...` binds the return value. A contract may call only pure functions.

```oska
fn halve(n: Int) -> Int
    requires n >= 0
    ensures |r| r <= n
{
    n / 2
}
```

Contracts are checked at runtime, in every build, including `--release`. A false `requires` stops the program with `P0006`, "precondition violated": it points at the call, because the caller broke the contract. A false `ensures` stops it with `P0007`, "postcondition violated": it points at the clause, and the message names the return that broke it, because the function broke its promise. Both quote the condition, exit with code 101, and fail only their own test under `oskac test`. `ensures` is checked on every way out of the function: falling off the end, an early `return`, and a `?` that returns an `Err` or `None`. `r` is that value.

A contract must be pure, and it must be a plain condition:

- A contract that calls a function with a capability is rejected (`E0280`).
- A `?` or `return` directly in a clause is rejected (`E0282`), because there is no function to leave. A closure inside a clause may use both for itself.
- If evaluating a clause panics, for example by overflow, that original panic is reported, not a contract violation.
- A function a contract calls keeps its own contract, so contracts compose.

A trait method's contract lives on the trait and binds every impl, whether the call goes through the trait, a qualified call such as `Shape.area(x)`, or a direct `x.area()`. A method of `impl Trait for Type` declares no contract of its own (`E0218`); move the clause to the trait. `main` declares none either (`E0219`), because nothing calls it. A method of an inherent `impl` can declare contracts.

```oska
trait Shape {
    fn scale(self, k: Int) -> Int
        requires k > 0
        ensures |r| r >= 0
}

struct Square { side: Int }

impl Shape for Square {
    fn scale(self, k: Int) -> Int {
        self.side * k
    }
}
```

## Concurrency

Tasks run inside a `scope |s| { ... }` block. `s.spawn(|| work())` starts a task and returns a handle, and `join()` returns its result. A scope does not end until all its tasks have finished, and the scope handle cannot escape the block. There is no `async` or `await`.

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    let total = scope |s| {
        let a = s.spawn(|| 20)
        let b = s.spawn(|| 22)
        a.join() + b.join()
    }
    io.println("{total}")?
    Ok(())
}
```

```console cmd=run
42
```

In this release, tasks run one at a time in the order they were spawned, so every run gives the same result.

## Modules and `std`

One file is one module, and items are private unless marked `pub`. `use` names a path with `.` as the separator, and only `std.*` modules can be imported so far. There are no glob imports. The modules are `std.sql`, `std.proc`, and `std.html`. Only `std.html` can run so far: the interpreter cannot run `std.sql` or `std.proc` yet, so `oskac run` refuses a program that imports one (`E0371`).

## Tests

A `test "name" { ... }` item is a top-level test with the built-in value `t`. See [testing](testing.md).

## What is not there yet

These are gaps in this release, so do not write programs that depend on them:

- A module-level `const` declares and checks, but its name cannot be used yet: the checker reports `E0221` for any reference to it.
- `Net`, `std.sql`, and `std.proc` check but do not run (`E0370`, `E0371`).
- Packages have no dependencies yet, no re-exports and no renaming of imports. See [packages and modules](packages.md).
- No formatter or package manager.
