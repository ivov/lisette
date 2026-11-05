---
title: "Types"
description: "Built-in types, annotations, parameters, aliases, conversion"
---

These types are built into the language and need no import.

| Type | See |
| --- | --- |
| `int` `int8` `int16` `int32` `int64` | [Numerics](/docs/prelude/numerics/) |
| `uint` `uint8` `uint16` `uint32` `uint64` `uintptr` | [Numerics](/docs/prelude/numerics/) |
| `byte` `rune` | [Numerics](/docs/prelude/numerics/) |
| `float32` `float64` `complex64` `complex128` | [Numerics](/docs/prelude/numerics/) |
| `bool` | [Booleans](/docs/prelude/booleans/) |
| `string` | [Strings](/docs/prelude/strings/) |
| `Option<T>` | [Option](/docs/prelude/option/) |
| `Result<T, E>` | [Result](/docs/prelude/result/) |
| `Partial<T, E>` | [Partial](/docs/prelude/partial/) |
| `Slice<T>` | [Slice](/docs/prelude/slice/) |
| `Array<T, N>` | [Array](/docs/prelude/array/) |
| `Map<K, V>` | [Map](/docs/prelude/map/) |
| `Ref<T>` | [Ref](/docs/prelude/ref/) |
| `Never` | [Never](/docs/prelude/types/#never) |
| `()` | [Unit](/docs/prelude/types/#unit) |
| `(A, B)` | [Tuples](/docs/lexemes/#tuples) |

## Type annotations

A type annotation states a type explicitly. Most types are inferred from the value. Annotations are placed after `:` on a binding or field.

```lisette
// !callout[/Slice<int>/] binding annotation
let empty: Slice<int> = []

struct Point {
  // !callout[/float64/] field annotation
  x: float64,
  y: float64,
}
```

Return type annotations are placed after `->`.

```lisette
// !callout-above[/Point/] return type annotation
fn origin() -> Point {
  Point { x: 0.0, y: 0.0 }
}
```

## Type parameters

A type parameter is a placeholder for a type. It is enclosed in angle brackets after the name of a function, method, struct, enum, or alias.

```lisette
fn first<T>(xs: Slice<T>) -> Option<T>
struct Pair<A, B> { left: A, right: B }
enum Tree<T> { Leaf(T), Node(Ref<Tree<T>>, Ref<Tree<T>>) }
```

Type arguments are given at the point of use, or inferred from the context.

```lisette
// !callout-right given
let scores = Map.new<string, int>()
// !callout-right inferred from annotation
let ages: Map<string, int> = Map.new()
```

### Bounds

A bound `T: Constraint` limits which types a type parameter accepts.

```lisette
interface Display {
  fn to_string() -> string
}
// !callout-above[/T: Display/] `T` must satisfy `Display`
fn print_value<T: Display>(value: T) {
  fmt.Println(value.to_string())
}
```

To give a type parameter multiple bounds:

```lisette
// !callout-center[/\+/] `T` must satisfy both\n`Display` and `Shape`
fn render<T: Display + Shape>(value: T) -> string
```

To give type parameters individual bounds:

```lisette
// !callout-center[/,/] `T` must satisfy `Display`\n`U` must satisfy `Ordered`
fn label<T: Display, U: Ordered>(value: T, rank: U) -> string
```

## Type aliases

To alias a type:

```lisette
type UserId = int
type Handler = fn(Request) -> Response
type StringMap<V> = Map<string, V>
```

Type aliases are transparent, i.e. alternative names for the same type.

```lisette
type UserId = int

let id: UserId = 42
// !callout-right `UserId` is just `int`
let n: int = id
```

For distinct types, use a [tuple struct](/docs/structs/#tuple-structs):

```lisette
struct UserId(int)
struct OrderId(int)

let user = UserId(1)
let order = OrderId(2)
// !callout-error-right error: expected `int`, found `UserId`
let n: int = user
```

## Type conversion

Types never convert implicitly, not even between numeric types. The `as` operator converts between types explicitly.

```lisette
let a: int = 1
let b: int64 = 2
// !callout-error-right error: cannot add `int` and `int64`
let c = a + b
```

```lisette
// !callout-center[/as/] explicit conversion
let c = a + (b as int)
```

Numeric conversions:

```lisette
let x: int = 42
let y = x as float64
let z = x as int8
```

Conversions of string to bytes or runes:

```lisette
let s = "hello"
let bytes = s as Slice<byte>
let runes = s as Slice<rune>
let back = bytes as string
```

Conversions between incompatible types are disallowed.

```lisette
let b = true
// !callout-error-right error: cannot convert `bool` to `int`
let n = b as int
```

## Types with zero values

These types have a zero value:

| Type | Zero value |
| --- | --- |
| numeric | `0` |
| `string` | `""` |
| `bool` | `false` |
| `Option<T>` | `None` |
| `Slice<T>` | `[]` |
| `Array<T, N>` | `N` zero values of `T` |
| enum with [default](/docs/attributes/#default) | default variant |

A struct or tuple has a zero value if all its component types have zero values.

```lisette
// !callout-right zero value is `Point { x: 0, y: 0 }`
struct Point { x: int, y: int }
// !callout-right zero value is `(0, "")`
type Pair = (int, string)
```

📚 See [`zero()`](/docs/prelude/functions/#zero) and [`Zeroable`](/docs/prelude/constraints/#zeroable)
