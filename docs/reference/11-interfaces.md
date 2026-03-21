# Interfaces

An interface is a set of method signatures that a type can implement.

```rust
interface Display {
  fn to_string(self) -> string
}
```

## Implementation

Lisette uses structural typing for interfaces, so a type automatically implements an interface by having all its methods:

```rust
struct Point {
  x: int,
  y: int,
}

impl Point {
  fn to_string(self) -> string {
    f"({self.x}, {self.y})"
  }  
}

// `Point` automatically implements `Display`
```

📚 See [`05-functions.md`](05-functions.md)

## Embedding

An interface can embed other interfaces, inheriting their methods:

```
interface Reader {
  fn read(self, buf: Slice<byte>) -> Result<int, error>
}

interface Writer {
  fn write(self, buf: Slice<byte>) -> Result<int, error>
}

interface ReadWriter {
  impl Reader
  impl Writer
}
```

A type implementing `ReadWriter` must have both `read` and `write` methods.

## Generic interfaces

Interfaces accept type parameters:

```rust
interface Iterator<T> {
  fn next(self) -> Option<T>
}

struct Range {
  current: int,
  end: int,
}

impl Range {
  fn next(self: Ref<Range>) -> Option<int> {
    if self.current >= self.end {
      return None
    }
    let n = self.current
    self.current += 1
    Some(n)
  }
}
```

## Invariance

Generic type parameters are invariant with respect to interface satisfaction. A `Box<Cat>` does not satisfy `Box<Animal>` even if `Cat` satisfies `Animal`, i.e. type args must match exactly.

```rust
interface Animal {
  fn name(self) -> string
}

struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn get(self) -> T { self.value }
}

fn take_box(b: Box<Animal>) { /* ... */ }

struct Cat {}
impl Cat {
  fn name(self) -> string { "cat" }
}

take_box(Box { value: Cat {} }) // error: expected `Box<Animal>`, found `Box<Cat>`
```

This matches Go's semantics, where generic type arguments are always invariant.

## Interfaces in generic types

An interface can be used as a type argument. For example, `Slice<Animal>` is a slice of interface values, so it can hold a collection of different types that all satisfy `Animal`:

```rust
let pets: Slice<Animal> = [Cat {}, Dog {}]
```

An interface can also be used as a generic bound. In this case, all elements are the same concrete type:

```rust
fn loudest<T: Animal>(pets: Slice<T>) -> T {
  pets.fold(pets[0], |a, b|
    if a.volume() > b.volume() { a } else { b }
  )
}

let cats = [Cat {}, Cat {}]
loudest(cats)  // T = Cat, returns Cat
```

<br>

<table><tr>
<td>← <a href="10-methods.md"><code>10-methods.md</code></a></td>
<td align="right"><a href="12-modules.md"><code>12-modules.md</code></a> →</td>
</tr></table>
