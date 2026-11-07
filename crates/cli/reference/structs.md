---
title: "Structs"
description: "Declaration, instantiation, write permission in fields, tuple structs, generic structs, embedding"
---

## Declaration

A struct groups named fields into one type.

```lisette
struct Point {
  x: int,
  y: int,
}
```

Structs and their fields can carry [attributes](/docs/attributes/).

## Instantiation

To instantiate a struct, provide all fields by name. To read a field, use dot notation:

```lisette
let p = Point { x: 10, y: 20 }
// !callout[/sum/] `30`
let sum = p.x + p.y
```

To write to a field, assign to it. The binding must be mutable:

```lisette
// !callout[/mut/] required by `p.x = 50`
let mut p = Point { x: 10, y: 20 }
p.x = 50
```

When a field name matches a binding in scope, the field value can be omitted:

```lisette
struct User {
  name: string,
  email: string,
  active: bool,
}

fn new_user(name: string, email: string) -> User {
  // !callout-right same as `name: name, email: email`
  User { name, email, active: true }
}
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

Each field declares its own write permission. A field whose type has `mut` is writable. A field without `mut` is read-only.

```lisette
struct Index {
  counts: mut Map<string, int>,
  tags: Slice<string>,
}
```

In a writable struct, `mut` fields accept writes:

```lisette
// !callout-above[/index/] writable struct
let mut index = Index {
  // !callout-right writable field
  counts: Map.new<string, int>(),
  // !callout-right read-only field
  tags: ["draft"],
}

// !callout-ok-right allowed: `index` and `counts` are both `mut`
index.counts["hits"] = 1
// !callout-error-right error: `tags` has no `mut`
index.tags[0] = "final"
```

In a read-only struct, no field accepts writes:

```lisette
// !callout-above[/snapshot/] read-only struct
let snapshot = Index {
  // !callout-right read-only field, because struct is read-only
  counts: Map.new<string, int>(),
  // !callout-right read-only field
  tags: ["draft"],
}

// !callout-error-right error: `snapshot` is not `mut`
snapshot.counts["hits"] = 1
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

A [type parameter](/docs/types/#type-parameters) lets one struct hold fields of any type. Lisette infers the type from the field values:

```lisette
struct Pair<T> { first: T, second: T }

// !callout-right `Pair<int>`
let nums = Pair { first: 1, second: 2 }
// !callout-right `Pair<string>`
let names = Pair { first: "alice", second: "bob" }
```

## Embedding

A struct can embed another struct. The fields and methods of the embedded struct are then available directly on the host struct:

```lisette
struct Logger {
  pub prefix: string,
}

impl Logger {
  pub fn log(self) -> string { self.prefix }
}

// !callout-above[/Server/] host struct
struct Server {
  // !callout-right embedded struct
  embed Logger,
  pub port: int,
}

let l = Logger { prefix: "[api]" }
let s = Server { Logger: l, port: 8080 }

// !callout-right host reads embedded field
s.prefix
// !callout-right host calls embedded method
s.log()
```
