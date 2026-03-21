# Methods

Methods are functions attached to a type, defined in `impl` blocks.

## `impl` blocks

An `impl` block groups methods for a type:

```rust
struct Rectangle {
  width: float64,
  height: float64,
}

impl Rectangle {
  fn area(self) -> float64 {
    self.width * self.height
  }

  fn scale(self: Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

Methods are called with dot notation:

```rust
let mut r = Rectangle { width: 10.0, height: 5.0 }
let a = r.area()  // 50.0
r.scale(2.0)
let b = r.area()  // 200.0
```

A type can have multiple `impl` blocks.

## Receivers

A method typically carries a `self` parameter, called the receiver.

A value receiver receives a copy:

```rust
impl Rectangle {
  fn area(self) -> float64 {
    self.width * self.height
  }
}
```

A reference receiver receives a reference (pointer) and can mutate:

```rust
impl Rectangle {
  fn scale(self: Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

## Auto-coercion

Methods can be called without adding `&` or `.*`:

```rust
let mut r = Rectangle { width: 10.0, height: 5.0 }
let ref = &r
let a = ref.area()  // auto-dereferenced
r.scale(2.0)        // auto-addressed
```

## Associated functions

A method without `self` is an associated function. It belongs to the type, not to an instance:

```rust
impl Rectangle {
  fn square(size: float64) -> Rectangle {
    Rectangle { width: size, height: size }
  }
}

let s = Rectangle.square(5.0)
```

## Generic methods

Methods can have their own type parameters. Here is `Option.map` from the prelude:

```rust
impl<T> Option<T> {
  fn map<U>(self, f: fn(T) -> U) -> Option<U> {
    match self {
      Some(x) => Some(f(x)),
      None => None,
    }
  }
}
```

The `impl` block introduces `T`, and the method introduces `U`.

```rust
let n = Some(42)
let s = n.map(|x| f"{x}")  // Option<string>
```

<br>

<table><tr>
<td>← <a href="09-error-handling.md"><code>09-error-handling.md</code></a></td>
<td align="right"><a href="11-interfaces.md"><code>11-interfaces.md</code></a> →</td>
</tr></table>
