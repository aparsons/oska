# Mocking

A mock replaces a real dependency with a fake that the test controls. Use mocks only where the real thing is slow, not repeatable, or outside your control.

## Mock only at system boundaries

A system boundary is a place where your code talks to something that it does not own. Mock these:

- external services, such as a payment API or an email provider;
- time, such as the current clock;
- randomness, such as a random token or a shuffle.

Sometimes mock these:

- the filesystem, when a real temporary directory is too slow or not portable;
- the database, when no real test database is available. Prefer a real test database.

In Oska, the host is the boundary. The `Host` trait in `crates/oska-run/src/host.rs` is the seam. Tests use `FakeHost` in place of `RealHost`. `FakeHost` already fixes the clock and the random seed, so do not add another fake for them. Use `RealHost` with a real temporary directory only when the test checks what the OS does, such as symlink resolution.

## Do not mock your own modules

A mock of your own module couples the test to the current split of the code. A refactor then breaks the test with no change in behaviour. Test your own modules through the seam with the real parts behind it.

## Design for injection

Pass each boundary dependency in. Do not create it inside the code.

```rust
pub trait Clock {
    fn now(&self) -> Instant;
}

pub struct RateLimiter<C: Clock> {
    clock: C,
    limit: u32,
    window: Duration,
}
```

The production code passes a real clock. The test passes a fixed clock that it moves forward by hand.

```rust
let clock = FixedClock::at(t(0));
let mut limiter = RateLimiter::new(clock.clone(), 3, Duration::from_secs(60));
clock.advance(Duration::from_secs(61));
```

## One function per external operation

Give each external operation its own function. Do not use one generic request function.

Bad. Each mock must parse a method and a path to know what to return.

```rust
trait Catalogue {
    fn request(&self, method: &str, path: &str, body: &str) -> Response;
}
```

Good. Each mock returns one simple value.

```rust
trait Catalogue {
    fn find_book(&self, isbn: &Isbn) -> Result<Option<Book>, CatalogueError>;
    fn reserve(&self, isbn: &Isbn, member: MemberId) -> Result<(), CatalogueError>;
}
```

A small trait also shows the full contract with the external service in one place.
