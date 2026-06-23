# Attributes

Attributes attach metadata or behavior to declarations.

## Serialization

Add `#[json]` to a struct to generate Go JSON struct tags for all fields:

```rust
#[json]
struct User {
  name: string,
  age: int,
  active: bool,
}
```

Generated Go:

```go
type User struct {
  Name   string `json:"name"`
  Age    int    `json:"age"`
  Active bool   `json:"active"`
}
```

Supported serialization attributes: `json`, `xml`, `yaml`, `toml`, `db`, `bson`, `mapstructure`, `msgpack`.

Serialization attributes accept options:

| Option       | Effect                           |
| ------------ | -------------------------------- |
| `omitempty`  | Omit field if empty              |
| `!omitempty` | Include field if empty           |
| `skip`       | Exclude field                    |
| `snake_case` | Convert field name to snake_case |
| `camel_case` | Convert field name to camelCase  |
| `string`     | Encode numbers as strings        |

```rust
#[json]
struct Config {
  #[json(omitempty)]
  timeout: Option<int>,

  #[json(skip)]
  internal_id: int,

  #[json(string)]
  large_number: int64,
}
```

Or override the serialized field name:

```rust
#[json]
struct User {
  #[json("user_id")]
  id: int,

  #[json("displayName")]
  name: string,
}
```

Struct-level options apply to all fields; field-level options override:

```rust
#[json(snake_case)]
struct UserProfile {
  userName: string,        // "user_name"

  createdAt: int,          // "created_at"

  #[json("userID")]
  uniqueId: int,           // "userID" (override)
}
```

Multiple attributes are supported:

```rust
#[json]
#[db]
struct User {
  #[json("userName")]
  #[db("user_name")]
  name: string,
}
```

## Custom tags

For custom tags, use `#[tag]`

```rust
#[json]
struct Input {
  #[tag("validate", "required")]
  email: string,
}
```

Generated Go:

```go
type Input struct {
  Email string `json:"email" validate:"required"`
}
```

For more complex tags, use a backticked string:

```rust
#[json]
struct User {
  #[tag(`validate:"required,email" gorm:"unique"`)]
  email: string,
}
```

Generated Go:

```go
type User struct {
  Email string `json:"email" validate:"required,email" gorm:"unique"`
}
```

## Lint suppression

`#[allow(lint)]` on a function silences that lint.

For most lints, place the attribute on the function whose code is flagged:

```rs
#[allow(match_on_bool)]
fn describe(ready: bool) -> string {
  match ready {
    true => "go",
    false => "wait",
  }
}
```

For the unused-value of lints (namely `unused_result`, `unused_option`, `unused_literal`, and `unused_value`), place `#[allow]` on the function whose result is ignored, so every call to it stops warning.

```rs
import "go:os"

#[allow(unused_result)]
fn warm_cache(path: string) -> Result<Slice<byte>, error> {
  os.ReadFile(path)
}

fn main() {
  warm_cache("/config")
  warm_cache("/data")
}
```

## Iteration

Add `#[iterate]` to an enum to synthesize a `variants()` associated function returning every variant, in declaration order:

```rs
#[iterate]
enum Direction {
  North,
  East,
  West,
  South,
}

for direction in Direction.variants() {
  fmt.Println(direction)
}
```

`Direction.variants()` returns a `Slice<Direction>`. Only enums whose variants have no payloads can be `#[iterate]`.

## Display

Add `#[display]` to a struct or enum to render it as a readable string when displayed.


```rs
#[display]
struct Point {
  x: int,
  y: int,
}

let p = Point { x: 1, y: 2 }

fmt.Println(p) // `Point { x: 1, y: 2 }`
```

Without `#[display]`, a struct or enum cannot be interpolated in an f-string, and displays using Go's `%v` default formatting.

```rs
fmt.Println(p) // `{1 2}` if Point is not `#[display]`
```

`#[display]` also gives the enum or struct a `to_string(self) -> string` method.

```rs
interface Display {
  fn to_string(self) -> string
}

fn render(value: Display) -> string {
  value.to_string()
}

render(Point { x: 1, y: 2 }) // `Point` satisfies `Display`
```

## Equality

`==` and `!=` work on natively comparable types: primitives and any struct, enum, and tuple whose components are all comparable.

```rs
struct User {
  name: string,
  age: int,
}

let u1 = User { name: "Alice", age: 30 }
let u2 = User { name: "Alice", age: 30 }

u1 == u2 // true
```

Other types are not natively comparable: slice and map and any struct, enum, and tuple that contains a slice or map, plus any function and interface value.

```rs
struct Order {
  id: int,
  tags: Slice<string>, // not natively comparable
}

let o1 = Order { id: 1, tags: ["a"] }
let o2 = Order { id: 1, tags: ["a"] }

o1 == o2 // compile error: comparison of non-comparables
```

For maps and slices, use the built-in `equals()` method:

```rs
let a = [1, 2, 3]
let b = [1, 2, 3]
let ok = a.equals(b) // true, element-wise
```

To enable comparison on types that are not natively comparable and do not have a built-in `equals()` method, mark them with the `#[equality]` attribute. This will auto-generate an `equals()` method to compare the type structurally.

```rs
#[equality]
struct Order {
  id: int,
  tags: Slice<string>,
}

let a = Order { id: 1, tags: ["a"] }
let b = Order { id: 1, tags: ["a"] }

a.equals(b) // true
```

The auto-generated `equals()` method compares by this rule: 

- `==` for comparable fields
- `.equals()` for slice and map fields
- the field type's own `equals` for nested `#[equality]` types

If you need a custom comparator, write an `equals` method yourself:

```rs
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

a.equals(b) // true
```

<br>

<table><tr>
<td>← <a href="14-concurrency.md"><code>14-concurrency.md</code></a></td>
<td align="right"><a href="16-testing.md"><code>16-testing.md</code></a> →</td>
</tr></table>
