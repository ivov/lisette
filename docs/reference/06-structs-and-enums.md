# Structs and Enums

## Structs

A struct groups related values into a single type.

```rust
struct Point {
  x: int,
  y: int,
}
```

Access fields with dot notation:

```rust
let p = Point { x: 10, y: 20 }
let sum = p.x + p.y
```

### Construction

To construct a struct, provide all fields by name:

```rust
let p = Point { x: 10, y: 20 }
```

When a variable matches a field name, the value can be omitted:

```rust
let x = 10
let y = 20
let p = Point { x, y }
```

Spread syntax copies fields from another instance, with explicit fields taking precedence:

```rust
let p1 = Point { x: 10, y: 20 }
let p2 = Point { x: 50, ..p1 }   // x: 50, y: 20
```

The zero-fill spread `..` fills any unspecified fields with their zero value:

```rust
let p = Point { x: 10, .. }   // y: 0
let q = Point { .. }          // x: 0, y: 0
```

### Tuple structs

A tuple struct has positional fields instead of named fields.

```rust
struct Color(int, int, int)
```

Construct with positional arguments:

```rust
let red = Color(255, 0, 0)
```

Access fields by index:

```rust
let r = red.0
let g = red.1
let b = red.2
```

### Generic structs

Structs accept type parameters:

```rust
struct Pair<T> {
  first: T,
  second: T,
}

let p = Pair { first: 1, second: 2 }
```

### Visibility

Fields are private by default. Use `pub` to make a field public:

```rust
struct User {
  pub name: string,
  email: string,
  password_hash: string,
}
```

Private fields can only be accessed within the same module. Use `pub struct` to make the type itself public. Structs and their fields can carry attributes.

📚 See [`12-modules.md`](12-modules.md) and [`15-attributes.md`](15-attributes.md)

## Enums

An enum defines a type with a fixed set of variants.

```rust
enum Direction {
  North,
  South,
  East,
  West,
}
```

To construct a variant, use the fully qualified name:

```rust
let dir = Direction.North
```

### Variants with payloads

Variants can carry data. A tuple variant has positional fields:

```rust
enum IpAddress {
  V4(int, int, int, int),
  V6(string),
}

let home = IpAddress.V4(127, 0, 0, 1)
let loopback = IpAddress.V6("::1")
```

A struct variant has named fields:

```rust
enum Shape {
  Circle { radius: float64 },
  Rectangle { width: float64, height: float64 },
}

let c = Shape.Circle { radius: 5.0 }
let r = Shape.Rectangle { width: 10.0, height: 20.0 }
```

Variants can be mixed in a single enum:

```rust
enum Event {
  Ready,
  KeyPress(rune),
  Click { x: int, y: int },
}
```

### Generic enums

Enums accept type parameters:

```rust
enum Cached<T> {
  Hit(T),
  Miss,
}

let found = Cached.Hit("hello")
```

### `Option` and `Result`

`Option` and `Result` are generic enums defined in the prelude:

```rust
enum Option<T> {
  Some(T),
  None,
}

enum Result<T, E> {
  Ok(T),
  Err(E),
}
```

Their variants need no prefix:

```rust
let name = Some("Alice") // same as `Option.Some("Alice")`
let missing = None

let ok = Ok(42) // same as `Result.Ok(42)`
let err = Err("oh no")
```

📚 See [`09-error-handling.md`](09-error-handling.md)

<br>

<table><tr>
<td>← <a href="05-functions.md"><code>05-functions.md</code></a></td>
<td align="right"><a href="07-pointers.md"><code>07-pointers.md</code></a> →</td>
</tr></table>
