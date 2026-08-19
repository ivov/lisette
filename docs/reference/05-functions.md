# Functions

A function has a name, parameters with type annotations, an optional return type, and a body.

```rust
fn add(a: int, b: int) -> int {
  a + b
}

fn greet(name: string) {
  fmt.Println(f"Hello, {name}")
}
```

Parameter types are required. The return type can be omitted; if so it defaults to `()`.

The last expression in the function body is the return value. Use `return` for early exits.

```rust
fn first_positive(nums: Slice<int>) -> Option<int> {
  for n in nums {
    if n > 0 {
      return Some(n)
    }
  }
  None
}
```

## Generic functions

Type parameters appear in angle brackets after the function name.

```rust
fn identity<T>(x: T) -> T {
  x
}

fn swap<A, B>(pair: (A, B)) -> (B, A) {
  (pair.1, pair.0)
}
```

The compiler infers type arguments at call sites:

```rust
let x = identity(42)         // T = int
let y = identity("hello")    // T = string
```

Explicit type arguments are needed when inference has nothing to work with:

```rust
let empty = Slice.new<int>()
let counts = Map.new<string, int>()
```

## Type bounds

A type parameter can carry a bound restricting which types may instantiate it. 

Two bounds are built in:

- `Comparable` for types that admit `==` and `!=` (arrays, structs, enums, and tuples qualify when all their components do)
- `Ordered` for types that admit `<`, `>`, `<=`, and `>=` (signed and unsigned integers, floats, and `string`)

`Ordered` implies `Comparable`, so `==` and `!=` are also available on a type bound by `Ordered`.

```rust
fn dedupe<T: Comparable>(xs: Slice<T>) -> Slice<T> { ... }
fn sorted<T: Ordered>(xs: Slice<T>) -> Slice<T> { ... }
```

A user-declared interface can also serve as a bound. See [`11-interfaces.md`](11-interfaces.md)

```rust
interface Display {
  fn to_string() -> string
}

fn print_value<T: Display>(value: T) {
  fmt.Println(value.to_string())
}
```

To a type parameter multiple bounds:

```rust
fn render<T: Display + Shape>(value: T) -> string { ... }
```

To give type parameters their individual bounds:

```rust
fn label<T: Display, U: Ordered>(value: T, rank: U) -> string { ... }
```

## Write permission in parameters

Parameters are immutable bindings. To mutate a local copy, rebind with `let mut`:

```rust
fn digits(n: int) -> int {
  let mut n = n
  let mut count = 1
  while n >= 10 {
    n /= 10
    count += 1
  }
  count
}
```

Write permission travels in the parameter's type. A parameter typed `mut Slice<T>`, `mut Map<K, V>`, or `mut Ref<T>` may write through to the caller's data, so the caller must pass a `mut` value:

```rust
fn reset_first(items: mut Slice<int>) {
  items[0] = 0             // writes observably to the caller
}

let mut nums = [3, 1, 2]
reset_first(nums)          // ok: `nums` permits writes

let frozen = [3, 1, 2]
reset_first(frozen)        // error: `Slice<int>` permits no write
```

## Lambdas

Anonymous functions whose params appear between `|` pipes.

```rust
let double = |x: int| x * 2
let sum = |a: int, b: int| a + b
let produce_int = || 42
```

Lambda parameter types can be omitted when inferable:

```rust
let doubled = [1, 2, 3].map(|x| x * 2)
```

A block body allows multiple statements:

```rust
let process = |x: int| {
  let y = x * 2
  y + 1
}
```

Lambdas capture variables from the enclosing scope:

```rust
let multiplier = 3
let scale = |x: int| x * multiplier
```

<br>

<table><tr>
<td>← <a href="04-control-flow.md"><code>04-control-flow.md</code></a></td>
<td align="right"><a href="06-structs-and-enums.md"><code>06-structs-and-enums.md</code></a> →</td>
</tr></table>
