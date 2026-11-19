# How should Oska implement an AI-first, secure-by-default `std.json`, and what do the best JSON parsers teach?

- **Date of access:** 2026-11-19
- **Scope:** Issue #248 "Agents: `std.json`". `json.parse` returns an untrusted value. `json.decode` turns it into a typed value and reports stable errors. `json.encode` writes canonical JSON. Limits on depth and size. Tests over JSON test suites. Both backends, all platforms.
- **Method:** 19 primary sources: three RFCs, the JSONTestSuite article and repository, one security study, and the docs or source of nine parsers. Repo context came from `README.md`, `GLOSSARY.md`, `docs/design.md`, ADR 0003, ADR 0006, the prelude, and `docs/research/2026-11-ai-first-secure-language.md`.
- **Prompt injection:** No source held text addressed to an AI reader.

## Short answer

Use a strict RFC 8259 parser with the I-JSON rules turned on. Reject duplicate names, invalid UTF-8, lone surrogates, a BOM, `NaN` and `Infinity`, comments, trailing commas and trailing content. Every limit is on by default: depth, input size, string length, number length, member count. Keep numbers as exact decimal text until a decode asks for a type. Write the parser in Oska, iteratively, so both backends share one implementation. Encode with RFC 8785 (JCS). Errors carry a stable code, a byte offset, a line and column, and a JSON Pointer.

## 1. What the standards say

### RFC 8259 leaves the hard cases open

- A JSON text is any serialized value, so a bare `3` or `"x"` is a valid text ([RFC 8259 §2](https://www.rfc-editor.org/rfc/rfc8259)).
- Object names "SHOULD be unique". When they are not, "the behavior of software that receives such an object is unpredictable". Many implementations keep only the last pair ([RFC 8259 §4](https://www.rfc-editor.org/rfc/rfc8259)).
- Integers in [-(2^53)+1, (2^53)-1] are interoperable. Implementations agree on their exact values because IEEE 754 binary64 is widely used ([RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259)).
- An implementation may limit the range and precision of numbers ([RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259)).
- The grammar allows an unpaired surrogate such as `"\uDEAD"`. Receivers then behave unpredictably ([RFC 8259 §8.2](https://www.rfc-editor.org/rfc/rfc8259)).
- JSON exchanged outside a closed ecosystem MUST be UTF-8 ([RFC 8259 §8.1](https://www.rfc-editor.org/rfc/rfc8259)).
- An implementation MUST NOT add a BOM to a networked JSON text. A parser MAY ignore one ([RFC 8259 §8.1](https://www.rfc-editor.org/rfc/rfc8259)).
- A parser MUST accept every text that matches the grammar. It MAY accept non-JSON extensions ([RFC 8259 §9](https://www.rfc-editor.org/rfc/rfc8259)).
- A parser may limit text size, nesting depth, and string length and content ([RFC 8259 §9](https://www.rfc-editor.org/rfc/rfc8259)).

Inference: RFC 8259 by itself does not give secure behavior. Every gap above is a place where two parsers can disagree.

### I-JSON (RFC 7493) closes most of the gaps

- Messages MUST be UTF-8. Strings and names MUST NOT contain surrogate or noncharacter code points ([RFC 7493 §2.1](https://www.rfc-editor.org/rfc/rfc7493)).
- Messages SHOULD NOT hold numbers with more magnitude or precision than binary64. Exact large integers are RECOMMENDED as strings ([RFC 7493 §2.2](https://www.rfc-editor.org/rfc/rfc7493)).
- Objects MUST NOT have duplicate names ([RFC 7493 §2.3](https://www.rfc-editor.org/rfc/rfc7493)).
- A protocol can require receivers to reject messages that break these rules, or, for security protocols, to not trust them ([RFC 7493 §3](https://www.rfc-editor.org/rfc/rfc7493)).
- Designers SHOULD NOT use top-level texts that are neither objects nor arrays ([RFC 7493 §4.1](https://www.rfc-editor.org/rfc/rfc7493)).
- Unrecognized members MUST be ignored ("Must-Ignore") ([RFC 7493 §4.2](https://www.rfc-editor.org/rfc/rfc7493)).
- Times are ISO 8601 strings. Binary data is base64url in a string ([RFC 7493 §4.3, §4.4](https://www.rfc-editor.org/rfc/rfc7493)).

### Canonical JSON (RFC 8785, JCS)

- The input must be I-JSON, with no duplicate names. Unicode normalization is not applied ([RFC 8785 §3.1](https://www.rfc-editor.org/rfc/rfc8785)).
- No whitespace is written between tokens ([RFC 8785 §3.2.1](https://www.rfc-editor.org/rfc/rfc8785)).
- Strings escape `"`, `\`, and U+0000 to U+001F. The short forms `\b \t \n \f \r` are used where they exist, and lowercase `\u00hh` elsewhere. Non-ASCII text is written as it is ([RFC 8785 §3.2.2.2](https://www.rfc-editor.org/rfc/rfc8785)).
- Numbers are written with the ECMAScript number-to-string rules ([RFC 8785 §3.2.2.3](https://www.rfc-editor.org/rfc/rfc8785)).
- `NaN` and `Infinity` MUST end canonicalization with an error. A lone surrogate is also an error ([RFC 8785 §3.2.2.2, §3.2.2.3](https://www.rfc-editor.org/rfc/rfc8785)).
- Object names are sorted by their UTF-16 code units, recursively. Array order is kept ([RFC 8785 §3.2.3](https://www.rfc-editor.org/rfc/rfc8785)).
- The purpose is repeatable hashing and signing ([RFC 8785 Abstract](https://www.rfc-editor.org/rfc/rfc8785)).
- Exact integers should stay within ±9007199254740991. Larger values should be strings ([RFC 8785 Appendix B, D](https://www.rfc-editor.org/rfc/rfc8785)).
- RFC 8785 is Informational, not standards track ([RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)).

Inference: JCS is a good fit for `json.encode`. It removes every free choice from the output, so the interpreter and native backends can be checked byte for byte. Its cost is the UTF-16 sort order, which is not the UTF-8 byte order for characters above U+FFFF.

## 2. What goes wrong in practice

### "Parsing JSON is a Minefield" and JSONTestSuite

- JSONTestSuite has more than 300 test files. `y_` files must be accepted, `n_` files must be rejected, and `i_` files are implementation defined ([Seriot](https://seriot.ch/projects/parsing_json.html); [JSONTestSuite README](https://github.com/nst/JSONTestSuite)).
- It also has `test_transform/` files for huge numbers, similar keys, NUL characters, and escaped invalid strings ([JSONTestSuite README](https://github.com/nst/JSONTestSuite)).
- Of more than 30 parsers tested, no two behaved the same ([Seriot](https://seriot.ch/projects/parsing_json.html)).
- Several parsers set no depth limit and crashed on deep nesting. Examples are cJSON, JSONKit and SBJSON ([Seriot](https://seriot.ch/projects/parsing_json.html)).
- `{"a":1,"a":2}` usually gives `{"a":2}`, but some parsers keep the first value or both. A 2017 update reports a CouchDB remote code execution caused by two parsers that handle duplicates differently ([Seriot](https://seriot.ch/projects/parsing_json.html)).
- For `["\uD800"]`, many parsers return an error although the grammar allows it. Python keeps the escape. Go and JavaScript substitute U+FFFD ([Seriot](https://seriot.ch/projects/parsing_json.html)).
- Parsers accepted non-JSON: Python accepts `NaN`, and one parser accepts trailing garbage such as `[]x` ([Seriot](https://seriot.ch/projects/parsing_json.html)).

### Parser differentials are a vulnerability class

- Bishop Fox tested 49 parsers in 10 languages ([Bishop Fox](https://bishopfox.com/blog/json-interoperability-vulnerabilities)).
- The attack classes it reports:
  - Duplicate-key precedence that differs between a validating service and a using service.
  - Key collision by truncation. One parser drops an invalid code point such as `\ud888`, so a crafted key becomes `superadmin`.
  - Comments that one parser reads and another ignores.
  - Large numbers that decode to 0, to a maximum value, to a rounded value, or to `Infinity`.
  - Trailing garbage accepted, which enabled CSRF bypasses.
  - Two parsers crashed with segmentation faults.
- Its advice for parser authors: fail on duplicate keys, do not truncate characters, keep a strict RFC 8259 mode, and fail on numbers that cannot be represented faithfully ([Bishop Fox](https://bishopfox.com/blog/json-interoperability-vulnerabilities)).

Inference: For Oska, the safe default is to reject every input on which parsers disagree. Then an Oska program can never be the "other parser" in a differential attack. Only one deviation from RFC 8259 §9 results: Oska rejects some texts that match the grammar (duplicates, lone surrogates). I-JSON allows that.

### Denial of service

- Python warns that a malicious JSON string "may cause the decoder to consume considerable CPU and memory resources". It recommends limiting input size ([Python json docs](https://docs.python.org/3/library/json.html)).
- Python's `json` sets no limits beyond those of Python's own types ([Python json docs](https://docs.python.org/3/library/json.html)).
- Converting decimal text to a big integer has no linear-time algorithm. Very long digit strings cost seconds of CPU. CVE-2020-10735 added a default limit of 4300 digits, and `json.loads` of a longer integer now raises `ValueError` ([CPython commit for CVE-2020-10735](https://mail.python.org/pipermail/python-checkins/2022-September/178585.html)).
- serde_json stops at a recursion depth of 128 by default ([serde_json `de.rs`](https://github.com/serde-rs/json/blob/master/src/de.rs)).
- serde_json's `unbounded_depth` feature warns that other recursive work can still overflow the stack after parsing, "including ... Display and Debug and Drop impls" ([serde_json `Cargo.toml`](https://github.com/serde-rs/json/blob/master/Cargo.toml)).
- RapidJSON has an iterative mode with "constant complexity in terms of function call stack size" ([RapidJSON DOM docs](https://github.com/Tencent/rapidjson/blob/master/doc/dom.md)).
- yyjson returns error `YYJSON_READ_ERROR_DEPTH` when nesting passes its depth limit ([yyjson API](https://github.com/ibireme/yyjson/blob/master/doc/API.md)).
- yyjson warns that its own `yyjson_equals` is recursive and may overflow the stack on deep values ([yyjson API](https://github.com/ibireme/yyjson/blob/master/doc/API.md)).
- Zig limits the length of one value (`max_value_len`). With a streaming reader, a default limit applies ([Zig `std/json/static.zig`](https://github.com/ziglang/zig/blob/master/lib/std/json/static.zig)).

Inference: A depth limit protects more than the parser. Oska frees values by reference counting (ADR 0001). Freeing a deeply nested value, comparing it, or printing it can recurse in the runtime. One limit at parse time bounds all of these.

## 3. What each parser teaches

| Parser | Lesson | Source |
|---|---|---|
| simdjson | Full validation and speed fit together. Stage 1 finds structure with SIMD and validates UTF-8. Stage 2 builds a "tape". | [paper](https://arxiv.org/html/1902.08318v7), [README](https://github.com/simdjson/simdjson) |
| simdjson | It rejects integers outside [-2^63, 2^63) and floats such as `1e309`. It does not round them silently. | [paper](https://arxiv.org/html/1902.08318v7) |
| simdjson | "A parser that accepts erroneous JSON is both dangerous" and poorly specified. | [paper](https://arxiv.org/html/1902.08318v7) |
| serde_json | Depth 128 by default. Exact float round-trip (`float_roundtrip`) costs about 2x on float parsing. Lossless numbers (`arbitrary_precision`) are opt-in. | [de.rs](https://github.com/serde-rs/json/blob/master/src/de.rs), [Cargo.toml](https://github.com/serde-rs/json/blob/master/Cargo.toml) |
| serde_json | Syntax errors carry a line and column. | [de.rs](https://github.com/serde-rs/json/blob/master/src/de.rs) |
| yyjson | The default mode is RFC 8259 strict. It rejects invalid UTF-8, a BOM, comments, trailing commas, `NaN` and `Inf`. Each extension is a separate opt-in flag. | [API](https://github.com/ibireme/yyjson/blob/master/doc/API.md) |
| yyjson | By default an integer that does not fit 64 bits becomes a `double`, so precision is lost unless `BIGNUM_AS_RAW` or `NUMBER_AS_RAW` is set. | [API](https://github.com/ibireme/yyjson/blob/master/doc/API.md) |
| yyjson | It has 16 numbered read error codes and a byte position. It can turn the position into line and column. | [API](https://github.com/ibireme/yyjson/blob/master/doc/API.md) |
| RapidJSON | Defaults are lax. UTF-8 validation and full-precision numbers are opt-in flags. The faster mode can be off by up to 3 ULP. | [DOM docs](https://github.com/Tencent/rapidjson/blob/master/doc/dom.md) |
| Go `encoding/json/v2` | It changes v1's defaults. It rejects invalid UTF-8 and duplicate names, and it matches field names case-sensitively. | [Go blog](https://go.dev/blog/jsonv2-exp), [pkg docs](https://pkg.go.dev/encoding/json/v2) |
| Go v2 | It splits syntax (`jsontext`, streaming, no reflection) from semantics (`json`, typed marshal and unmarshal). Options are an argument to every call. | [Go blog](https://go.dev/blog/jsonv2-exp) |
| Go v2 | `SemanticError` gives a byte offset, a JSON Pointer (RFC 6901) and the Go type. `RejectUnknownMembers` is opt-in. | [pkg docs](https://pkg.go.dev/encoding/json/v2) |
| Go v2 | Concrete integer types keep precision. Decoding into `any` gives `float64` and can lose precision. | [pkg docs](https://pkg.go.dev/encoding/json/v2) |
| Python `json` | Lax by default. It accepts `NaN` and `Infinity`, keeps the last duplicate, and accepts lone surrogates. It rejects a BOM. `JSONDecodeError` has `msg`, `pos`, `lineno` and `colno`. | [docs](https://docs.python.org/3/library/json.html) |
| Zig `std.json` | Typed parsing rejects duplicate fields (`error.DuplicateField`) and unknown fields (`error.UnknownField`) by default. Both have opt-out options. | [static.zig](https://github.com/ziglang/zig/blob/master/lib/std/json/static.zig) |
| jiter | It has three interfaces: a value tree, a pull iterator that the caller drives with an expected schema, and direct conversion to Python objects. Big integers have their own number variant. | [README](https://github.com/pydantic/jiter) |
| sonic-rs | It parses straight into the target type with no intermediate tape. It has a `LazyValue` (a raw, valid JSON slice) and a `RawNumber`. Its fastest field lookups are `unsafe` and skip validation. | [README](https://github.com/cloudwego/sonic-rs) |

Inference: The parsers built before about 2015 (RapidJSON, Python, Go v1) chose lax defaults and added strict flags. Newer designs (yyjson, Go v2, Zig) make strict the default and lax the opt-in. Oska should follow the newer pattern and not offer the lax flags at all.

Inference: The fastest parsers either change the API (lazy values, pull iterators) or skip validation behind `unsafe`. Oska needs neither for version 1. A scalar, validating parser in Oska is enough to start. SIMD speed can come later behind the same API.

## 4. Typed decoding: parse, don't validate

- Zig parses straight into a static type and fails on a duplicate or unknown field ([Zig `static.zig`](https://github.com/ziglang/zig/blob/master/lib/std/json/static.zig)).
- jiter's iterator API makes the caller state the type it expects at each step, for example `next_str()` and `next_int()` ([jiter README](https://github.com/pydantic/jiter)).
- Go v2 reports where a typed decode failed (JSON Pointer) and which type it wanted ([Go v2 pkg docs](https://pkg.go.dev/encoding/json/v2)).
- Oska has no user macros. ADR 0003 names "built-in, compiler-defined derivations for a fixed set of traits (equality, hashing, serialization)" as the answer to boilerplate (`docs/adr/0003-no-user-defined-macros.md`).
- In Oska, a `String` field means trusted text. Code that takes raw input must declare `Untrusted<String>`. Converting to a non-text type makes a value trusted (ADR 0006, `GLOSSARY.md`).

Inference: `json.decode` is a validator in the glossary's sense. It turns an untrusted JSON value into a typed value. A field of type `Int`, `Bool`, or an enum becomes trusted by conversion, as ADR 0006 already says. A text field must be declared `Untrusted<String>`. If a struct field is a plain `String`, decode must refuse at check time with a stable code. The fix moves the text through a validator that makes a trust assertion. Otherwise `json.decode` would be a hidden trust assertion, and ADR 0006 forbids that.

## 5. Error messages an agent can act on

- Python gives a message, a byte position, a line and a column ([Python json docs](https://docs.python.org/3/library/json.html)).
- yyjson gives a numbered error code and a byte position. Line and column count Unicode characters ([yyjson API](https://github.com/ibireme/yyjson/blob/master/doc/API.md)).
- Go v2 gives a byte offset, a JSON Pointer, and the target type ([Go v2 pkg docs](https://pkg.go.dev/encoding/json/v2)).
- Oska diagnostics and panics already have stable codes and precise spans, and are emitted as JSON with `--format=json` (`docs/design.md`).
- Oska's `IoError.message` is `Untrusted<String>` (`crates/oska-check/src/prelude.osk`).

Inference: A JSON error should hold: a stable code, the byte offset, the line and column, the JSON Pointer of the value being decoded, the expected and actual kind, and the limit that was hit with its value. Any piece of the input quoted in the message is untrusted text, as in `IoError`. A short excerpt with control characters escaped is safe to print and still helps an agent.

## 6. Recommended design for Oska

Inference: everything in this section is a recommendation drawn from the findings above. It is not existing behavior.

### 6.1 API shape

```oska
use std.json

// Syntax layer: bytes or text to a JSON value tree. All text in it is untrusted.
json.parse(text: Untrusted<String>) -> Result<json.Value, json.Error>
json.parse_with(text: Untrusted<String>, limits: json.Limits) -> Result<json.Value, json.Error>

// Semantic layer: a JSON value to a type with a built-in derivation.
json.decode<T: json.Decode>(value: json.Value) -> Result<T, json.Error>

// Encoding: always RFC 8785 canonical output.
json.encode<T: json.Encode>(value: T) -> Result<String, json.Error>
```

- `json.Value` is a sum type: `Null`, `Bool(Bool)`, `Number(json.Number)`, `String(Untrusted<String>)`, `Array(List<json.Value>)`, `Object(json.Object)`.
- `json.Object` keeps members in input order. Names are unique, so lookup is unambiguous.
- `json.Number` holds the validated decimal text. It has `as_int() -> Result<Int, json.Error>` and later `as_bigint()` and a float accessor. It never rounds silently. This follows simdjson, I-JSON §2.2, and the Bishop Fox advice.
- Add a `decode_text<T>(text, limits)` shortcut that parses and decodes in one step. It can later skip the tree, as sonic-rs and Go v2 do, with the same results.
- `json.Decode` and `json.Encode` are compiler-derived traits under ADR 0003. They are not user macros.
- Field names match exactly and case-sensitively, as in Go v2.

### 6.2 Strict parsing, with no lax options

| Input | Behavior |
|---|---|
| Duplicate object name | Error. The check uses the decoded names, after escapes, so `"a"` and `"a"` are duplicates. |
| Invalid UTF-8 | Error. No replacement and no truncation. |
| Escaped lone surrogate, e.g. `"\uD800"` | Error (I-JSON §2.1). Oska text is valid Unicode, so it cannot hold one anyway. |
| Noncharacters, e.g. U+FFFF | Open question. I-JSON forbids them, RFC 8259 allows them. |
| BOM at the start | Error, as in Python and yyjson. |
| `NaN`, `Infinity`, comments, trailing commas, single quotes, unquoted names | Error. |
| Content after the value | Error, except trailing whitespace. |
| Top-level scalar | Accepted, as RFC 8259 §2 requires. `decode` into a struct then fails with a type error. |
| Unknown member in `decode` | Error by default, as in Zig. A struct opts in to I-JSON's Must-Ignore with a declared attribute. |
| Missing member | Error, unless the field type is `Option<T>`. |
| `null` | Decodes only into `Option<T>`. |

### 6.3 Limits on by default

| Limit | Default | Reason |
|---|---|---|
| Nesting depth | 128 | The serde_json default. It also bounds recursive drop, equality and printing. |
| Input size | 16 MiB | Python's advice to limit size. The exact value is an open question. |
| String length, in bytes | the input size | Zig's `max_value_len` default for a slice. |
| Number length, in characters | 100 | Far above any binary64 or 64-bit value. It stops big-number CPU attacks like CVE-2020-10735. |
| Members per object | 10 000 | Bounds duplicate checks and hashing. |
| Total values | 1 000 000 | Bounds memory for small inputs with many tiny values. |

- `json.Limits` is a plain struct with these defaults. A caller can raise a limit only by naming it in the source, so a reviewer can see it.
- Hitting a limit gives an error with its own code, the limit's name, and its value. It is never a panic.

### 6.4 Implementation

- Write the parser in Oska, in `std`. The interpreter and the native backends then run the same code. This fits ADR 0011 and ADR 0012, which put such work in Oska and not in Rust.
- Parse iteratively with an explicit stack of at most `depth` frames, as RapidJSON's iterative mode does. Never recurse on input structure.
- Parse numbers in two steps. First check the grammar and length and keep the text. Convert only on `as_int()` or on decode.
- Validate UTF-8 once, over the whole input, before tokenizing, as simdjson's stage 1 does. Add an ASCII fast path first and SIMD later.
- `encode` sorts names by UTF-16 code units (JCS §3.2.3). It escapes strings as JCS §3.2.2.2 says. It writes integers as plain digits.
- Encoding an `Int` outside ±(2^53 − 1) is an error with a stable code that suggests `String` or `BigInt`-as-string. This follows JCS Appendix B and I-JSON §2.2.

### 6.5 Error codes

- Use one stable family, for example `J0001`..`J00nn`. Split them into syntax (unexpected byte, bad escape, bad number, trailing content), I-JSON (duplicate name, invalid UTF-8, lone surrogate, BOM), limits (one code for each limit), decode (wrong kind, missing member, unknown member, integer out of range, text needs `Untrusted<String>`), and encode (integer out of safe range).
- Each error has: `code`, `offset` (bytes), `line`, `column` (code points, as Oska's `len()` counts), `pointer` (RFC 6901), `expected`, `found`, and `message: Untrusted<String>`.
- `oskac` prints an uncaught JSON error in the same JSON shape as other diagnostics.

### 6.6 Tests

- Run every JSONTestSuite `y_` file (must parse) and `n_` file (must fail). Pin each `i_` file to an outcome in a table in the repo. Most become "reject" under §6.2.
- Run the `test_transform/` files and check that each one is either rejected or round-trips exactly.
- Run the RFC 8785 sample inputs and outputs for `encode`.
- Add limit tests at the limit and one past it, for each limit. Include 100 000 nested `[` and a 1 MB number.
- Fuzz with differential checks: interpreter against native, on all platforms, for the same verdict, error code, offset and output bytes. This is the parity gate in issue #248.

## 7. Open questions

- **Noncharacters.** Should `parse` reject U+FFFE, U+FFFF and the others, as I-JSON §2.1 says? Some real data contains them.
- **Floats.** The prelude has no float type today (`crates/oska-check/src/prelude.osk`). `Number` can wait for one. Output then needs a shortest round-trip algorithm to match JCS's ECMAScript rules. RFC 8785 recommends Ryu or V8's algorithm.
- **Unknown members.** Should they be rejected by default, as in Zig, or ignored, as I-JSON §4.2 and Go v2 do? Rejecting is safer. Ignoring makes protocols easier to extend.
- **Default limits.** 16 MiB and 10 000 members are guesses. No source gives a standard value except serde_json's depth of 128 and Python's 4300 digits. Measure with the eval harness and real data.
- **Bytes input.** Should `parse` also take `Untrusted<Bytes>`? Trust on `Bytes` is planned but not built yet (`docs/design.md`).
- **Pretty output.** JCS gives no indentation. Do agents need a readable, non-canonical `encode_pretty`? It would add a second output format to keep in step across backends.
- **Streaming.** Go v2 and jiter stream. Does Oska need a pull parser in version 1, or only the value tree?
- **Go v2 depth limit.** The Go v2 package docs I read do not state a nesting limit. I did not confirm Go's value.
- **JSONTestSuite results today.** The Seriot article is from 2016, updated 2017. Current results for each parser were not checked.

## Sources

1. RFC 8259, The JSON Data Interchange Format: https://www.rfc-editor.org/rfc/rfc8259
2. RFC 7493, The I-JSON Message Format: https://www.rfc-editor.org/rfc/rfc7493
3. RFC 8785, JSON Canonicalization Scheme (JCS): https://www.rfc-editor.org/rfc/rfc8785
4. Nicolas Seriot, Parsing JSON is a Minefield: https://seriot.ch/projects/parsing_json.html
5. JSONTestSuite README: https://github.com/nst/JSONTestSuite
6. Bishop Fox, An Exploration of JSON Interoperability Vulnerabilities: https://bishopfox.com/blog/json-interoperability-vulnerabilities
7. Langdale and Lemire, Parsing Gigabytes of JSON per Second: https://arxiv.org/html/1902.08318v7
8. simdjson README: https://github.com/simdjson/simdjson
9. serde_json source, `src/de.rs`: https://github.com/serde-rs/json/blob/master/src/de.rs
10. serde_json `Cargo.toml` (feature docs): https://github.com/serde-rs/json/blob/master/Cargo.toml
11. yyjson API docs: https://github.com/ibireme/yyjson/blob/master/doc/API.md
12. RapidJSON DOM docs: https://github.com/Tencent/rapidjson/blob/master/doc/dom.md
13. Go blog, A new experimental Go API for JSON: https://go.dev/blog/jsonv2-exp
14. Go `encoding/json/v2` package docs: https://pkg.go.dev/encoding/json/v2
15. Python `json` module docs: https://docs.python.org/3/library/json.html
16. CPython commit for CVE-2020-10735 (int/str digit limit): https://mail.python.org/pipermail/python-checkins/2022-September/178585.html
17. Zig `std.json` source, `lib/std/json/static.zig`: https://github.com/ziglang/zig/blob/master/lib/std/json/static.zig
18. jiter README: https://github.com/pydantic/jiter
19. sonic-rs README: https://github.com/cloudwego/sonic-rs
