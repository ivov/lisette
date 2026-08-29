---
title: "Structs"
description: "Declaration, instantiation, write permission in fields, tuple structs, generic structs, embedding"
---

## Declaration

A struct groups named fields into one type.

```lisette
struct Point {
  // !callout-right field `x`, type `int`
  x: int,
  // !callout-right field `y`, type `int`
  y: int,
}
```

Structs and their fields can carry [attributes](/docs/attributes/).

## Instantiation

To instantiate a struct, provide all fields by name. To read a field, use dot notation:

```lisette
let p = Point { x: 10, y: 20 }
// !callout-right `30`
let sum = p.x + p.y
```

To write a field, assign to it through a mutable binding:

```lisette
let mut p = Point { x: 10, y: 20 }
p.x = 50
```

When a field name matches a binding in scope, the field value can be omitted:

```lisette
let x = 10
let y = 20
// !callout-right `x: 10`, `y: 20`
let p = Point { x, y }
```

To copy field values from one instance to another, use `..` followed by the source instance. Explicit fields take precedence:

```lisette
let p1 = Point { x: 10, y: 20 }
// !callout-right `x: 50`, `y: 20`
let p2 = Point { x: 50, ..p1 }
```

Use a bare `..` to autofill remaining fields with their zero value:

```lisette
// !callout-right `x: 10`, `y: 0`
let p = Point { x: 10, .. }
// !callout-right `x: 0`, `y: 0`
let q = Point { .. }
```

## Write permission in fields

Each field declares its own write permission.

```lisette
struct Index {
  // !callout-right writable, through a writable `Index`
  counts: mut Map<string, int>,
  // !callout-right read-only
  tags: Slice<string>,
}
```

A field is writable only through a writable struct.

```lisette
let mut first = Index {
  counts: Map.new<string, int>(),
  tags: ["lisette"],
}
// !callout-ok-right writable struct, writable field
first.counts["hits"] = 1
// !callout-error-right error: `first.tags` is read-only
first.tags[0] = "go"

let second = Index {
  counts: Map.new<string, int>(),
  tags: ["lisette"],
}
// !callout-error-right error: `second` was declared without `mut`
second.counts["hits"] = 1
```

## Tuple structs

A tuple struct has positional fields instead of named fields.

```lisette
struct Color(int, int, int)
```

To instantiate a tuple struct:

```lisette
let red = Color(255, 0, 0)
```

Access fields by position:

```lisette
// !callout-right `255`
let r = red.0
// !callout-right `0`
let g = red.1
// !callout-right `0`
let b = red.2
```

## Generic structs

Structs accept type parameters:

```lisette
struct Pair<T> {
  first: T,
  second: T,
}

// !callout-right `Pair<int>`
let numbers = Pair { first: 1, second: 2 }
// !callout-right `Pair<string>`
let names = Pair { first: "alice", second: "bob" }
```

## Embedding

A struct can embed another struct. This composes the embedded struct's methods and fields into the host struct:

```lisette
struct Logger {
  pub prefix: string,
}

impl Logger {
  pub fn log(self) -> string { self.prefix }
}

// !callout-right host struct
struct Server {
  // !callout-right embedding
  embed Logger,
  pub port: int,
}

let l = Logger { prefix: "[api]" }
let s = Server { Logger: l, port: 8080 }

// !callout-right host reads embedded field
let _ = s.prefix
// !callout-right host calls embedded method
let _ = s.log()
```
