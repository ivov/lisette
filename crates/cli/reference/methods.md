---
title: "Methods"
description: "impl blocks, receivers, associated functions, generic methods"
---

Methods are functions attached to a type, defined in `impl` blocks.

## `impl` blocks

An `impl` block groups methods for a type:

```lisette
struct Rectangle {
  width: float64,
  height: float64,
}

impl Rectangle {
  fn area(self) -> float64 {
    self.width * self.height
  }

  fn scale(self: mut Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

Methods are called with dot notation:

```lisette
let mut rect = Rectangle {
  width: 10.0,
  height: 5.0,
}

// !callout-right `50.0`
let before = rect.area()
rect.scale(2.0)
// !callout-right `200.0`
let after = rect.area()
```

A type can have multiple `impl` blocks.

## Receivers

A method on an instance takes a `self` parameter, called the receiver.

A value receiver reads a copy:

```lisette
impl Rectangle {
  // !callout[/Rectangle/] copy of `Rectangle` instance
  fn area(self: Rectangle) -> float64 {
    self.width * self.height
  }
}
```

A `Ref` receiver reads the original:

```lisette
impl Rectangle {
  // !callout[/Ref/] read-only pointer to `Rectangle` instance
  fn perimeter(self: Ref<Rectangle>) -> float64 {
    2.0 * (self.width + self.height)
  }
}
```

A `mut Ref` receiver writes to the original:

```lisette
impl Rectangle {
  // !callout[/Ref/] writable pointer to `Rectangle` instance
  fn scale(self: mut Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

Prefer the value receiver for a small struct, and `Ref` for a large one, where the copy
is costly. See [Go's guidance](https://go.dev/wiki/CodeReviewComments#receiver-type) on
receiver types.

A value receiver may omit the type:

```lisette
impl Rectangle {
  // !callout[/self/] same as `self: Rectangle`
  fn area(self) -> float64 {
    self.width * self.height
  }
}
```

Calling a method never needs an explicit `&` or `.*`, because Lisette [auto-adds them](/docs/references/#implicit--and--in-method-calls) as needed.

## Associated functions

A method without `self` is an associated function. It belongs to the type, not to an instance:

```lisette
impl Rectangle {
  fn square(size: float64) -> Rectangle {
    Rectangle { width: size, height: size }
  }
}

// !callout[/(?<=\.)square/] called on the type itself, not on an instance
let squared = Rectangle.square(5.0)
```

## Generic methods

A method can define type parameters of its own:

```lisette
// !callout-right `T` is the type inside `Option`
impl<T> Option<T> {
  // !callout-right `U` is what `f` turns `T` into
  fn map<U>(self, f: fn(T) -> U) -> Option<U> {
    match self {
      Some(value) => Some(f(value)),
      None => None,
    }
  }
}
```

```lisette
let count = Some(42)
// !callout-right `Option<string>`
let label = count.map(|n| f"{n}")
```

