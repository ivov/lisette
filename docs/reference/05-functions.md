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

A type parameter can be constrained to types that implement an interface. See [`11-interfaces.md`](11-interfaces.md)

```rust
fn print_value<T: Display>(value: T) {
  fmt.Println(value.to_string())
}
```

Multiple bounds use `+`:

```rust
fn process<T: Display + Clone>(value: T) -> T {
  fmt.Println(value.to_string())
  value.clone()
}
```

Multiple type parameters can each have their own bounds:

```rust
fn combine<T: Display, U: Debug>(a: T, b: U) -> string {
  a.to_string() + b.debug_string()
}
```

Two built-in bounds are predeclared:

- `Comparable` (in the prelude) for types that admit `==` and `!=`.
- `cmp.Ordered` (in `go:cmp`) for types that admit `<`, `>`, etc.

```rust
import "go:cmp"

fn dedupe<T: Comparable>(xs: Slice<T>) -> Slice<T> { /* ... */ }
fn sort<T: cmp.Ordered>(xs: Slice<T>) { /* ... */ }
```

## Mutable parameters

Parameters are immutable by default. If a function mutates a parameter (e.g. sorting a slice in place), mark it with `mut`:

```rust
fn sort_in_place(mut items: Slice<int>) {
  // ...
}

let mut mutable_nums = [3, 1, 2]
sort_in_place(mutable_nums)  // ok

let immutable_nums = [3, 1, 2]
sort_in_place(immutable_nums)  // error
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
