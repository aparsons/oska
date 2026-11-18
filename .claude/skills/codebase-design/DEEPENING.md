# Deepening a module

When you merge shallow modules into a deeper one, decide how to test it. The kind of each dependency decides the test plan.

## Dependency categories

| Category | Example | Test plan |
|---|---|---|
| **In-process** | Pure code, in-memory data structures. In Oska: the parser in `oska-syntax`, the checker passes. | Merge it into the module. Test the module directly. |
| **Local stand-in exists** | A database with a local test instance, a temporary directory. In Oska: `FakeHost` for the OS, `Scratch` from `portable.rs` for real paths. | Test with the stand-in. |
| **Owned remote service** | A service that your team runs. | Define a port. Write a production adapter and an in-memory adapter. Test with the in-memory adapter. |
| **Third-party service** | A payment API, an email provider. In Oska: a git host that serves packages (ADR 0010). | Define a port. Write a production adapter and a mock adapter. Test with the mock adapter. |

A **port** is the interface that the module needs from the outside world. Adapters fill it.

## Seam discipline

- A seam with one adapter is only indirection. Add the seam when the second adapter, such as a test adapter, exists.
- Internal seams stay internal. Do not show a seam inside the module through its public interface.
- Keep the port small. One function for each external operation is easier to fill than one generic request function.

## Testing the deeper module

- Replace the old tests on the shallow parts with tests at the new interface.
- Assert on outcomes that callers see, not on calls between the inner parts.
- The new tests must survive a refactor behind the interface.
- Delete an old test only when a new test at the interface covers its behaviour.
