---
title: "References"
description: "Referencing, dereferencing, mutation, implicit & and .*"
---

[`Ref<T>`](/docs/prelude/ref/) points to a value of type `T`. It compiles to a [Go pointer](https://go.dev/ref/spec#Pointer_types) `*T` but is never nil.

## Referencing

`&` takes a reference to a value that has an address: a variable, a field, or an element.

```lisette
let user = User { name: "Alice", tags: ["admin"] }

// !callout-right `Ref<User>` pointing to `user`
let user_ref = &user
// !callout-right `Ref<string>` pointing to `user.name`
let name_ref = &user.name
// !callout-right `Ref<string>` pointing to `user.tags[0]`
let tag_ref = &user.tags[0]
```

Literals, map values, and constants have no address.

```lisette
const LIMIT = 10
let ages = Map.new<string, int>()

// !callout-error-right error: cannot take address of literal
let a = &42
// !callout-error-right error: cannot take address of map index expression
let b = &ages["Bob"]
// !callout-error-right error: `const` bindings are not addressable
let c = &LIMIT
```

## Dereferencing

`.*` reads the value that a reference points to:

```lisette
// !callout-right `"Alice"`
let name = name_ref.*
// !callout-right `"admin"`
let tag = tag_ref.*
```

## Mutation

Only a `mut Ref<T>` accepts writes. To get one, take `&` of a writable value:

```lisette
fn bump(n: mut Ref<int>) {
  n.* += 1
}

let mut count = 1
let total = 1

// !callout-ok-right `&count` is a `mut Ref<int>`
bump(&count)
// !callout-error-right error: expected `mut Ref<int>`, found `Ref<int>`
bump(&total)
```

## Implicit `&` and `.*` in method calls

In a [method](/docs/methods/) call, Lisette auto-adds `&` or `.*` to the receiver as needed:

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
let rect_ref = &rect

// !callout-right same as `rect_ref.*.area()`
rect_ref.area()
// !callout-right same as `(&rect).scale(2.0)`
rect.scale(2.0)
```

