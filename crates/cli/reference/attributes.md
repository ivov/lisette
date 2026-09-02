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

`omitempty` has no effect on a struct, enum, tuple, or `Array<T, N>` field with `N` greater than zero. None of them is one of Go's [empty values](https://pkg.go.dev/encoding/json#Marshal), so use `omitzero` for those.

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

Add `#[iterate]` to an enum to synthesize a `variants()` associated function returning every variant, in declaration order:

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

let p = Point { x: 1, y: 2 }

// !callout-right prints `{1 2}`
fmt.Println(p)
// !callout-error-right error: `Point` has no display form
fmt.Println(f"at {p}")
```

Add `#[display]` to render it as a readable string instead.

```lisette
#[display]
struct Point {
  x: int,
  y: int,
}

let p = Point { x: 1, y: 2 }

// !callout-right prints `Point { x: 1, y: 2 }`
fmt.Println(p)
```

`#[display]` also gives the enum or struct a `to_string()` method.

```lisette
interface Display {
  fn to_string() -> string
}

fn render(value: Display) -> string {
  value.to_string()
}

// !callout-right `Point` satisfies `Display`
render(Point { x: 1, y: 2 })
```

### `#[equality]`

`==` and `!=` work on natively comparable types: primitives, arrays whose elements are comparable, and structs, enums, and tuples whose components are all comparable.

```lisette
struct User {
  name: string,
  age: int,
}

let u1 = User { name: "Alice", age: 30 }
let u2 = User { name: "Alice", age: 30 }

// !callout-right `true`
u1 == u2
```

Other types are not natively comparable: slices, maps, functions, interfaces, and any array, struct, enum, or tuple that contains a non-comparable value.

```lisette
struct Order {
  id: int,
  // !callout-right not natively comparable
  tags: Slice<string>,
}

let o1 = Order { id: 1, tags: ["a"] }
let o2 = Order { id: 1, tags: ["a"] }

// !callout-error-right error: `Order` cannot be compared with `==`
o1 == o2
```

For maps and slices, use the built-in `equals()` method:

```lisette
let a = [1, 2, 3]
let b = [1, 2, 3]

// !callout-right `true`
a.equals(b)
```

To enable comparison on types that are not natively comparable and do not have a built-in `equals()` method, mark them with the `#[equality]` attribute. This will auto-generate an `equals()` method to compare the type structurally.

```lisette
#[equality]
struct Order {
  id: int,
  tags: Slice<string>,
}

let a = Order { id: 1, tags: ["a"] }
let b = Order { id: 1, tags: ["a"] }

// !callout-right `true`
a.equals(b)
```

The auto-generated `equals()` method compares by these rules:

- `==` for comparable fields
- `.equals()` for slice and map fields
- the field type's own `equals` for nested `#[equality]` types

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

let a = Fraction { numerator: 1, denominator: 2 }
let b = Fraction { numerator: 2, denominator: 4 }

// !callout-right `true`
a.equals(b)
```

`#[equality]` works on generic structs and enums, as long as the type parameter is bound by `Comparable` or `Ordered`.

```lisette
#[equality]
struct Cart<T: Comparable> {
  items: Slice<T>
}

let a = Cart { items: [1, 2, 3] }
let b = Cart { items: [1, 2, 3] }

// !callout-right `true`
a.equals(b)
```

The bound can also be a custom interface instead, as long as the interface declares an `equals` method.

```lisette
interface Equatable<T> {
  fn equals(other: T) -> bool
}

#[equality]
struct Batch<T: Equatable<T>> {
  item: T
}

let a = Batch { item: Order { id: 1, tags: ["a"] } }
let b = Batch { item: Order { id: 1, tags: ["a"] } }

// !callout-right `true`
a.equals(b)
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
```

## `#[allow]`

`#[allow(lint)]` on a function silences that lint.

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

For unused-value lints (`unused_result`, `unused_option`, `unused_literal`, `unused_value`), place `#[allow]` on the function whose result is ignored, so every call to it stops warning.

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
  South,
}

fn main() {
  let _ = Direction.North
}
```
