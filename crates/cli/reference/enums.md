---
title: "Enums"
description: "Declaration, variants with data, generic enums"
---

## Declaration

An enum defines a type with a fixed set of variants.

```lisette
enum Direction {
  North,
  East,
  West,
  South,
}
```

## Variants with data

Variants can carry data. A tuple variant has positional fields:

```lisette
enum IpAddress {
  V4(int, int, int, int),
  V6(string),
}

let home = IpAddress.V4(127, 0, 0, 1)
let loopback = IpAddress.V6("::1")
```

A struct variant has named fields:

```lisette
enum Shape {
  Circle { radius: float64 },
  Rectangle { width: float64, height: float64 },
}

let c = Shape.Circle { radius: 5.0 }
let r = Shape.Rectangle { width: 10.0, height: 20.0 }
```

Variants can be mixed in a single enum:

```lisette
enum Event {
  Ready,
  KeyPress(rune),
  Click { x: int, y: int },
}
```

## Generic enums

Enums accept type parameters:

```lisette
enum Cache<T> {
  Hit(T),
  Miss,
}

let found = Cache.Hit("hello")
```

`Option`, `Result`, and `Partial` in the [prelude](/docs/prelude/) are generic enums.

