---
title: "References"
description: "Referencing, dereferencing, mutation, implicit & and .*"
---

[`Ref<T>`](/docs/prelude/ref/) is a reference to a value of type `T`, equivalent to a [Go pointer](https://go.dev/ref/spec#Pointer_types) but guaranteed non-nil.

## Referencing

Use `&` to take a reference to anything that has an address, such as a variable, a field, or an element. Literals, map values, and `const` bindings have none.

```lisette
let user = User { name: "Alice", tags: ["admin"] }

// !callout-right `Ref<User>` pointing to `user`
let user_ref = &user
// !callout-right `Ref<string>` pointing to `user.name`
let name_ref = &user.name
// !callout-right `Ref<string>` pointing to `user.tags[0]`
let tag_ref = &user.tags[0]
```

## Dereferencing

Use `.*` to follow a reference back to the pointee:

```lisette
// !callout-right `"Alice"`
let name = name_ref.*
// !callout-right `"admin"`
let tag = tag_ref.*
```

## Mutation

Only a `mut Ref<T>` can be written to:

```lisette
fn bump(n: mut Ref<int>) {
  n.* += 1
}

let mut count = 1
// !callout-ok-right `&count` is a `mut Ref<int>`
bump(&count)

let total = 1
// !callout-error-right error: expected `mut Ref<int>`, found `Ref<int>`
bump(&total)
```

Refs only grant what the place they point to allows. `&x` is a `mut Ref<T>` if `x` allows writes, else a plain `Ref<T>`.

## Implicit `&` and `.*` in method calls

When calling [methods](/docs/methods/), Lisette auto-adds `&` or `.*` as needed.

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
  }
}

let mut rect = Rectangle { width: 10.0, height: 5.0 }
let shape = &rect

// !callout-right equivalent to `shape.*.area()`
let a = shape.area()
// !callout-right equivalent to `(&rect).scale(2.0)`
rect.scale(2.0)
```

