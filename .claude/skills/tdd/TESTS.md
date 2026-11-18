# Good and bad tests

The examples use two small domains. A rate limiter allows a fixed number of calls per time window. A library loan service lends books to members. The code is Rust, but the rules apply to every language.

## Behaviour, not implementation

Bad. The test reads an internal field. A change to the storage breaks it, but the behaviour stays the same.

```rust
#[test]
fn records_call() {
    let mut limiter = RateLimiter::new(3, Duration::from_secs(60));
    limiter.try_acquire("alice", t(0));
    assert_eq!(limiter.buckets["alice"].len(), 1); // private detail
}
```

Good. The test states a capability through the public interface.

```rust
#[test]
fn refuses_the_fourth_call_in_one_window() {
    let mut limiter = RateLimiter::new(3, Duration::from_secs(60));
    for _ in 0..3 {
        assert!(limiter.try_acquire("alice", t(0)).is_ok());
    }
    assert!(limiter.try_acquire("alice", t(1)).is_err());
}
```

## Through the interface, not a side channel

Bad. The test checks the result with a direct database query. It skips the interface that callers use.

```rust
#[test]
fn lending_marks_book_out() {
    let db = test_db();
    let loans = LoanService::new(&db);
    loans.lend(BOOK_42, MEMBER_7).unwrap();
    let row = db.query_one("SELECT status FROM books WHERE id = 42");
    assert_eq!(row.get::<String>("status"), "out");
}
```

Good. The test asks the service the same question a caller asks.

```rust
#[test]
fn a_lent_book_is_not_available() {
    let loans = LoanService::new(&test_db());
    loans.lend(BOOK_42, MEMBER_7).unwrap();
    assert_eq!(loans.availability(BOOK_42), Availability::OnLoan);
}
```

## Independent literal, not a recomputed value

Bad. The expected due date uses the same formula as the code. If the formula is wrong, the test is wrong too.

```rust
#[test]
fn due_date() {
    let loan = lend_on(date(2026, 3, 1));
    assert_eq!(loan.due, date(2026, 3, 1) + Duration::days(LOAN_DAYS));
}
```

Good. The expected value is a literal that a person checked by hand.

```rust
#[test]
fn a_loan_on_1_march_is_due_on_22_march() {
    let loan = lend_on(date(2026, 3, 1));
    assert_eq!(loan.due, date(2026, 3, 22));
}
```

## Red flags

Look again at a test when you see one of these signs:

- The test name says how the code works, not what it does.
- The test calls a private function or reads a private field.
- The test mocks a module that this repo owns.
- The expected value comes from a constant or a formula in the code under test.
- One refactor with no behaviour change breaks many tests.
- The test checks a result through a database query, a log line, or a file that callers do not read.
- The test passes before you write the code.
- The test needs a long setup that hides what it checks.
- The test depends on the real clock, real randomness, or the order of other tests.
