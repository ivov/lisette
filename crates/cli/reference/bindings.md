---
title: "Bindings"
description: "let, let mut, write permission, const, annotating, destructuring"
---

## `let`

`let` creates an immutable binding. `let` permits no writes to the value, and no reassignment of the name.

```lisette
let defaults = [10, 20, 30]
let timeout = 30

// !callout-error-right write not permitted
defaults[0] = 60
// !callout-error-right reassignment not permitted
timeout = 60
```

## `let mut`

`let mut` creates a mutable binding. `let mut` permits writes to the value, and reassignment of the name.

```lisette
let mut scores = [90, 85, 77]
let mut count = 0

// !callout-ok-right write permitted
scores[0] = 100
// !callout-ok-right reassignment permitted
count += 1
```

## Write permission in bindings

`let mut` encodes write permission in the value's type:

```lisette
// !callout[/scores/] `scores` is a `mut Slice<int>`\nand not merely a `Slice<int>`
let mut scores = [90, 85, 77]
```

In Go, `[]T`, `map[K]V` and `*T` allow two names to share the same data, so a write through one can change what the other sees. Lisette makes that risk visible in the type: `Slice`, `Map` and `Ref` are writable only when the type carries `mut`.

With write permission visible in the type, a function announces if it can write to its parameter, and a struct field announces if it can be written to.

📚 See [Write permission in parameters](/docs/functions/#write-permission-in-parameters) and [Write permission in fields](/docs/structs/#write-permission-in-fields)

Write permission can shrink but never grow. A name that shares data inherits the other's permission, even under `let mut`.

```lisette
let a = [1, 2, 3]
// !callout-ok-right reassignment allowed, but `b` inherits `a`'s read-only permission
let mut b = a
// !callout-error-right error: `b` is read-only
b[0] = 99

// !callout-right independent copy, free to write
let mut c = a.clone()
// !callout-ok-right write permitted, `a` untouched
c[0] = 99
```

## `const`

`const` defines a compile-time constant. Only primitive values are allowed: `bool`, `int`, `float64`, `string`. The initializer must be a literal or an expression built from literals. `const` bindings are immutable and unaddressable.

```lisette
const MAX_SIZE = 1024
const GREETING = "hello"
const DOUBLED = MAX_SIZE * 2
```

A tuple, struct, `Slice`, or `Array` cannot be `const`. Use a function that returns the value instead:

```lisette
fn origin() -> Point {
  Point { x: 0, y: 0 }
}
```

## Annotating

Add a type annotation with `:` after the binding name. On a binding it is optional, since the type usually follows from the value, unlike a [function parameter](/docs/functions/), where it is required.

```lisette
let x: int = 42
let y = 42
```

## Destructuring

A binding can name the parts of a value instead of the whole. Write the shape of the value on the left, called a [pattern](/docs/pattern-matching/), and each name in it binds to the part in that position. Tuples, structs, and enums can all be taken apart this way.

```lisette
// !callout-right `x` is `10`, `y` is `20`
let (x, y) = (10, 20)
let Point { x, y } = point
let Shape.Circle(radius) = shape
```
