# Testing

Lisette's test runner finds `#[test]` functions in `.test.lis` files and runs them with `lis test`.

## Running tests

`lis test` compiles the project and runs every test in it.

In the test report, tests are grouped by module and file:

```
  ✓ Compiled `demo` v0.1.0 (120ms)

  math
    math.test.lis
      ├── ✓ addition
      ├── ✓ subtraction
      ├── ✓ multiplication
      └── ✓ division

  ✓ 4 passed (104ms)
```

On failure, the test report includes a `Failures` section:

```
  ✓ Compiled `demo` v0.1.0 (120ms)

  math
    math.test.lis
      ├── ✕ addition
      ├── ✓ subtraction
      ├── ✓ multiplication
      └── ✓ division

  Failures

  ✕ addition · expected ==
       ╭─[src/math/math.test.lis:3:10]
     1 │ #[test]
     2 │ fn addition() {
     3 │   assert add(2, 2) == 5
       ·          ───────┬──────
       ·                 ╰── left: 4 · right: 5
     4 │ }
     5 │
       ╰────

  ✕ 1 failed · 3 passed (104ms)
```

`lis test` exits non-zero when any test fails, or when a run finishes without having executed any test.

## Test files

Test files end with `.test.lis` and sit in the same module as the logic they cover, so tests can access the module's private symbols directly.

```
src/
├── main.lis
└── math/
    ├── math.lis         # logic under test
    └── math.test.lis    # tests internal to math module
```

`lis check` includes test files and production code. `lis build` and `lis run` exclude test files from the binary.

Tests external to a module are not supported yet.

## Test functions

In a test file, a function marked `#[test]` is found and invoked by the test runner. A test function usually has no parameters and no return type.

```rs
#[test]
fn addition() {
  assert add(2, 2) == 4
}
```

Optionally, a test can also carry a title and description:

```rs
/// Surrounding whitespace is trimmed before the count is taken.
#[test("counts fields in a CSV row")]
fn counts_fields() {
  assert field_count("  a , b ") == 3
}
```

The function title replaces the function name in the report, and the description follows beneath:

```
  Failures

  ✕ counts fields in a CSV row · expected ==
    Surrounding whitespace is trimmed before the count is taken.
       ╭─[src/math/math.test.lis:4:10]
     2 │ #[test("counts fields in a CSV row")]
     3 │ fn counts_fields() {
     4 │   assert field_count("  a , b ") == 3
       ·          ──────────────┬─────────────
       ·                        ╰── left: 2 · right: 3
     5 │ }
       ╰────
```

`lis test --filter <pattern>` matches against both function names and function titles.

## Assertions

In a test function, the `assert` keyword checks if a boolean expression is `true` or fails the test.

```rs
#[test]
fn basics() {
  assert is_email("name@example.com")
  assert is_email("not-an-email")
}
```

On failure:

```
  Failures

  ✕ basics · assertion failed
       ╭─[src/math/math.test.lis:4:10]
     2 │ fn basics() {
     3 │   assert is_email("name@example.com")
     4 │   assert is_email("not-an-email")
       ·          ────────────┬───────────
       ·                      ╰── assertion failed
     5 │ }
       ╰────
```

To compare types that are not comparable with `==`, mark them with `#[equality]` and use the `equals` method. See [Equality](15-attributes.md#equality).

```rs
#[equality]
struct Order {
  id: int,
  tags: Slice<string>,
}

#[test]
fn orders_match() {
  let a = Order { id: 1, tags: ["a"] }
  let b = Order { id: 9, tags: ["z"] }
  assert a.equals(b)
}
```

On failure:

```
  Failures

  ✕ orders_match · expected ==
        ╭─[src/math/math.test.lis:11:10]
      9 │   let a = Order { id: 1, tags: ["a"] }
     10 │   let b = Order { id: 9, tags: ["z"] }
     11 │   assert a.equals(b)
        ·          ─────┬─────
        ·               ╰─┤ left:  Order { id: 1, tags: ["a"] }
        ·                 │ right: Order { id: 9, tags: ["z"] }
     12 │ }
        ╰────
```

Use `let assert` to assert by pattern matching:

```rs
#[test]
fn parses_header() {
  let bytes: Slice<byte> = [0x02, 0x00]
  let assert Ok(h) = parse_header(bytes) // mismatch fails the test
  assert h.version == 2
}
```

To assert that an expression panics, `recover` from it and match the `Err`.

```rs
#[test]
fn panics_out_of_bounds() {
  let xs = [1, 2, 3]
  let assert Err(_) = recover { xs[9] } // non-panic fails the test
}
```

Use `Result` to assert via the `?` operator:

```rs
#[test]
fn round_trips() -> Result<(), error> {
  let point = Point { x: 1, y: 2 }
  let bytes = encode(point)? // `Err` fails the test
  let restored = decode(bytes)? // `Err` fails the test
  assert restored == point
  Ok(())
}
```

## Test context

A test can take a `t` parameter of type `TestContext`, which works similarly to Go's `testing.T`.

For example, use `t.run` to group assertions into named subtests:

```rs
fn cases() -> Slice<Case> {
  [
    Case { name: "a", input: 1, expected: 2 },
    Case { name: "b", input: 2, expected: 4 },
  ]
}

#[test]
fn basics(t: TestContext) {
  for case in cases() {
    t.run(case.name, |_| {
      assert compute(case.input) == case.expected
    })
  }
}
```

If omitted, the type of `t` is inferred:

```rs
#[test]
fn basics(t) {
  for case in cases() {
    t.run(case.name, |_| {
      assert compute(case.input) == case.expected
    })
  }
}
```

`t` is also available as a lambda parameter:

```rs
#[test]
fn basics(t) {
  for case in cases() {
    t.run(case.name, |t| {
      t.parallel() // mark concurrent
      assert compute(case.input) == case.expected
    })
  }
}

```

`t.skip()` stops a test early and records why.

```rs
#[test]
fn status_ok(t: TestContext) {
  if !online() {
    t.skip("service offline")
  }
  assert fetch_status() == 200
}
```

On skipping:

```
  math
    math.test.lis
      └── ○ status_ok (service offline)

  ✓ 1 skipped (104ms)
```

`t.log(value)` displays a value for debugging:

```rs
#[test]
fn computes_total(t: TestContext) {
  let total = add(20, 22)
  t.log(total)
  assert total == 42
}
```

Logged values appear in a `Logs` section:

```
  Logs

  ≡ computes_total
       ╭─[src/math/math.test.lis:4:9]
     2 │ fn computes_total(t: TestContext) {
     3 │   let total = add(20, 22)
     4 │   t.log(total)
       ·         ──┬──
       ·           ╰── 42
     5 │   assert total == 42
     6 │ }
       ╰────
```

## Go test flags

Use `--go-flags` to pass flags through to `go test`.

```sh
lis test --go-flags "-failfast"
```

Useful flags:

| Flag           | Effect                                        |
| -------------- | --------------------------------------------- |
| `-failfast`    | Stop at the first failing test                |
| `-race`        | Enable the data race detector                 |
| `-timeout 30s` | Fail the run if it exceeds the given duration |

To select tests by name, use `lis test --filter` rather than `-run`.

<br>

<table><tr>
<td>← <a href="15-attributes.md"><code>15-attributes.md</code></a></td>
</tr></table>
