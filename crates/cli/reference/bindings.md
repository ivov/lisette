---
title: "Bindings"
description: "let, let mut, write permission, const, annotating, destructuring"
---

## Read-only bindings

`let` creates a read-only binding. `let` disallows writes to the value and reassignment of the name.

```lisette
let defaults = [10, 20, 30]
let timeout = 30

// !callout-error-right error: write not allowed
defaults[0] = 60
// !callout-error-right error: reassignment not allowed
timeout = 60
```

## Mutable bindings

`let mut` creates a mutable binding. `let mut` allows writes to the value and reassignment of the name.

```lisette
let mut scores = [90, 85, 77]
let mut count = 0

// !callout-ok-right write allowed
scores[0] = 100
// !callout-ok-right reassignment allowed
count += 1
```

## Write permission in bindings

In Go, `[]T`, `map[K]V` and `*T` allow two names to share the same data, so a write through one can change what the other sees. Lisette makes that risk visible in the type: `let mut` encodes write permission in the value's type.

`Slice`, `Map` and `Ref` are writable only when the type carries `mut`.

```lisette
// !callout-right `mut Slice<int>`
let mut scores = [90, 85, 77]
// !callout-ok-right write allowed
scores[0] = 100

// !callout-right `Slice<int>`
let ranks = [1, 2, 3]
// !callout-error-right error: write not allowed
ranks[0] = 5
```

📚 See also [Write permission in parameters](/docs/functions/#write-permission-in-parameters) and [Write permission in fields](/docs/structs/#write-permission-in-fields)

A name that shares data cannot have more write permission than the original.

```lisette
// !callout-right read-only
let a = [1, 2, 3]
// !callout-ok-right reassignment allowed, but read-only like `a`
let mut b = a
// !callout-error-right error: `b` is read-only
b[0] = 99

// !callout-right independent copy
let mut c = a.clone()
// !callout-ok-right write allowed, `a` unchanged
c[0] = 99
```

## Constant bindings

`const` defines a compile-time constant. Its type must be numeric, `bool`, or `string`, and its initializer can use only literals and other constants. A constant has no address.

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

## Annotating a binding

Annotate a type with `:` after the binding name. The annotation is optional, because the type usually follows from the value. A [function parameter](/docs/functions/) always needs one.

```lisette
let x: int = 42
let y = 42
```

## Destructuring a binding

A binding can take a value apart with a [pattern](/docs/pattern-matching/). Each name in the pattern binds to the part in its position.

```lisette
let t = (10, 20)
// !callout-right `a` is `10`, `b` is `20`
let (a, b) = t

let p = Point { x: 3, y: 4 }
// !callout-right `x` is `3`, `y` is `4`
let Point { x, y } = p
```
