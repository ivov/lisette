---
title: "Attributes"
description: "Serialization and custom tags, iteration, display, equality, default variants, lint suppression"
---

Attributes attach metadata or behavior to declarations.

## Struct tags

### `#[json]`

Add `#[json]` to a struct to generate [Go JSON struct tags](https://pkg.go.dev/encoding/json#Marshal) for all fields:

```lisette
#[json]
struct User {
  name: string,
  age: int,
  active: bool,
}
```

Generated Go:

```go file="user.go"
type User struct {
  Name   string `json:"name"`
  Age    int    `json:"age"`
  Active bool   `json:"active"`
}
```

Serialization attributes accept options:

```lisette
#[json]
struct Config {
  #[json(omitempty)]
  // !callout-right omitted if `None`
  timeout: Option<int>,

  #[json(skip)]
  // !callout-right never serialized
  internal_id: int,

  #[json(string)]
  // !callout-right encoded as `"1234"`, not `1234`
  large_number: int64,
}
```

| Option       | Effect                                 |
| ------------ | -------------------------------------- |
| `omitempty`  | Omit field if empty                    |
| `!omitempty` | Include field if empty                 |
| `omitzero`   | Omit field if it holds a zero value    |
| `!omitzero`  | Include field if it holds a zero value |
| `skip`       | Exclude field                          |
| `snake_case` | Convert field name to snake_case       |
| `camel_case` | Convert field name to camelCase        |
| `string`     | Encode numbers as strings              |

Struct-level options apply to all fields. A field-level option overrides them, and an explicit name overrides both:

```lisette
#[json(snake_case)]
struct UserProfile {
  // !callout-right `"user_name"` from `snake_case`
  userName: string,

  // !callout-right `"created_at"` from `snake_case`
  createdAt: int,

  #[json("userID")]
  // !callout-right `"userID"`, `snake_case` overridden
  id: int,
}
```

`omitempty` does not omit a struct, enum, tuple, or non-empty `Array<T, N>` field, because Go never treats these as [empty](https://pkg.go.dev/encoding/json#Marshal). Use `omitzero` for these fields.

### Other serializers

More supported attributes:

```lisette
#[xml]
#[yaml]
#[toml]
#[db]
#[bson]
#[mapstructure]
#[msgpack]
```

Attributes stack:

```lisette
#[json]
#[db]
struct User {
  #[json("userName")]
  #[db("user_name")]
  name: string,
}
```

### `#[tag]`

For custom tags, use `#[tag]`:

```lisette
#[json]
struct Input {
  #[tag("validate", "required")]
  email: string,
}
```

Generated Go:

```go file="input.go"
type Input struct {
  Email string `json:"email" validate:"required"`
}
```

For more complex tags, use a backticked string:

```lisette
#[json]
struct User {
  #[tag(`validate:"required,email" gorm:"unique"`)]
  email: string,
}
```

Generated Go:

```go file="user.go"
type User struct {
  Email string `json:"email" validate:"required,email" gorm:"unique"`
}
```

## Generated code

### `#[iterate]`

`#[iterate]` gives an enum an [associated function](/docs/methods/#associated-functions) `variants()`, which returns a `Slice` of all variants in declaration order:

```lisette
#[iterate]
enum Direction {
  North,
  East,
  West,
  South,
}

for direction in Direction.variants() {
  // !callout-right prints `North, East, West, South`
  fmt.Println(direction)
}
```

`#[iterate]` works only on enums whose variants carry no data.

### `#[display]`

By default a struct or enum has no display form. Printing one falls back to [Go's `%v`](https://pkg.go.dev/fmt#hdr-Printing), and interpolating one in an f-string is rejected.

```lisette
struct Point {
  x: int,
  y: int,
}

fn log_position(p: Point) {
  // !callout-right prints `{1 2}`
  fmt.Println(p)
}
```

Add `#[display]` to render it as a readable string instead.

```lisette
#[display]
struct Point {
  x: int,
  y: int,
}

fn log_position(p: Point) {
  // !callout-right prints `Point { x: 1, y: 2 }`
  fmt.Println(p)
}
```

`#[display]` also gives the enum or struct a `to_string()` method.

```lisette
fn label(p: Point) -> string {
  // !callout-right `"Point { x: 1, y: 2 }"`
  p.to_string()
}
```

### `#[equality]`

`==` and `!=` work only on [`Comparable`](/docs/prelude/constraints/#comparable) types:

- primitives
- structs, enums, tuples, and arrays whose components are all `Comparable`

```lisette
struct User {
  // !callout-right `Comparable`
  name: string,
  // !callout-right `Comparable`
  age: int,
}
```

```lisette
let u1 = User { name: "Alice", age: 30 }
let u2 = User { name: "Alice", age: 30 }

// !callout-right `true`
u1 == u2
```

Maps and slices are not `Comparable`. Instead, they have a built-in `equals()` method, which compares structurally:

- slices are compared element by element
- maps are compared by keys and values

```lisette
let s1 = [1, 2, 3]
let s2 = [1, 2, 3]

// !callout-right `true`
s1.equals(s2)
```

A struct with a field that is not `Comparable` is not `Comparable` either:

```lisette
struct Order {
  // !callout-right `Comparable`
  id: int,
  // !callout-right not `Comparable`
  tags: Slice<string>,
}
```

```lisette
let o1 = Order { id: 1, tags: ["a"] }
let o2 = Order { id: 1, tags: ["a"] }

// !callout-error-right error: `Order` cannot be compared with `==`
o1 == o2
```

To compare a struct or enum that is not `Comparable`, mark it with `#[equality]`. This attribute generates an `equals()` method that compares structurally, like the built-in one on maps and slices.

```lisette
#[equality]
struct Order {
  id: int,
  tags: Slice<string>,
}
```

```lisette
let o1 = Order { id: 1, tags: ["a"] }
let o2 = Order { id: 1, tags: ["a"] }

// !callout-right `true`
o1.equals(o2)
```

The generated `equals()` method compares each field by these rules:

- `==` for fields that support `==`
- `equals()` for slice and map fields
- the field type's own `equals()` for nested `#[equality]` types

If you need a custom comparator, write an `equals` method yourself:

```lisette
struct Fraction {
  numerator: int,
  denominator: int,
}

impl Fraction {
  fn equals(self, other: Fraction) -> bool {
    self.numerator * other.denominator == other.numerator * self.denominator
  }
}
```

```lisette
let f1 = Fraction { numerator: 1, denominator: 2 }
let f2 = Fraction { numerator: 2, denominator: 4 }

// !callout-right `true`
f1.equals(f2)
```

`#[equality]` works on generic structs and enums, as long as the type parameter is bound by `Comparable`, `Ordered`, or an interface that declares `equals()`.

```lisette
#[equality]
struct Cart<T: Comparable> {
  items: Slice<T>
}
```

```lisette
let c1 = Cart { items: [1, 2, 3] }
let c2 = Cart { items: [1, 2, 3] }

// !callout-right `true`
c1.equals(c2)
```

## `#[default]`

Out of the box, an enum has no zero value.

```lisette
enum Status {
  Active,
  Paused,
  Stopped,
}

// !callout-error-right error: `Status` has no zero value
let queue = Slice.make<Status>(2)
// !callout-error-right error: `Status` has no zero value
let slots = Array.new<Status, 3>()
```

Place `#[default]` on an enum variant to make it the zero value.

```lisette
enum Status {
  Active,
  Paused,
  #[default]
  Stopped,
}

// !callout-right `[Stopped, Stopped]`
let queue = Slice.make<Status>(2)
// !callout-right `[Stopped, Stopped, Stopped]`
let slots = Array.new<Status, 3>()
```

An enum's default appears wherever a zero value is expected.

```lisette
struct Job {
  pub name: string,
  pub status: Status,
}

// !callout[/job/] `job.status` is `Status.Stopped`
let job = Job { name: "build", .. }

let by_name = Map.new<string, Status>()
// !callout[/missing/] `Status.Stopped`
let missing = by_name["absent"]

// !callout[/initial/] `Status.Stopped`
let initial = zero<Status>()
```

## `#[allow]`

`#[allow(lint)]` silences a lint. Where to place it depends on the lint. To silence several lints, list them: `#[allow(unused_struct_field, unused_type)]`.

For most lints, place the attribute on the function whose code is flagged:

```lisette
// !callout-right silences the lint for this function
#[allow(match_on_bool)]
fn describe(ready: bool) -> string {
  match ready {
    true => "go",
    false => "wait",
  }
}
```

For unused-value lints (`unused_result`, `unused_option`, `unused_literal`, `unused_value`), place `#[allow]` on the function whose result is ignored, so every call to it stops warning. An `#[allow]` on the calling function has no effect.

```lisette
import "go:os"

// !callout-right silences the lint at every call below
#[allow(unused_result)]
fn warm_cache(path: string) -> Result<Slice<byte>, error> {
  os.ReadFile(path)
}

fn main() {
  warm_cache("/config")
  warm_cache("/data")
}
```

For the unused-item lints (`unused_function`, `unused_type`, `unused_struct_field`, `unused_enum_variant`), place `#[allow]` on the flagged declaration itself. An allow on a struct or enum also covers its fields and variants.

```lisette
#[allow(unused_enum_variant)]
enum Direction {
  North,
  // !callout-right silences the lint for `South`
  South,
}

fn main() {
  let _ = Direction.North
}
```
