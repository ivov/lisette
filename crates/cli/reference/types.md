---
title: "Types"
description: "Built-in types, annotations, parameters, aliases, conversion"
---

## Built-in types

These types come from the language itself, so they need no import.

### Prelude types

Each of these is declared in the [prelude](/docs/prelude/).

| Type | Prelude section |
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

### Tuple

Tuples hold 2 to 5 values, which may have different types. Access elements by position.

```lisette
let pair = (42, "hello")
// !callout-right `42`
let first = pair.0
// !callout-right `"hello"`
let second = pair.1

let triple = (1, true, "three")
// !callout-right `a` is `1`, `b` is `true`, `c` is `"three"`
let (a, b, c) = triple
```

For more than 5 elements, use a [struct](/docs/structs/) with named fields.

### Unit

The implicit return type of functions that return no value. Written as `()`.

```lisette
// !callout-right implies `-> ()`
fn greet(name: string) {
  fmt.Println(f"hello, {name}")
}
```

## Type annotations

A type annotation names a type the compiler cannot work out on its own. Write it after `:` on a binding, parameter, or field, and after `->` for a return type.

```lisette
// !callout-error-right error: element type cannot be inferred
let empty = []
// !callout-right annotation decides element type
let empty: Slice<int> = []
```

Most types are inferred from the value, so annotations are the exception.

## Type parameters

A type parameter stands for a type supplied later, when the declaration is used. Functions, methods, structs, enums, and aliases all take type parameters, written in angle brackets after the name.

```lisette
fn first<T>(xs: Slice<T>) -> Option<T>
struct Pair<A, B> { left: A, right: B }
enum Tree<T> { Leaf(T), Node(Ref<Tree<T>>, Ref<Tree<T>>) }
```

Supply them at the point of use, or leave them out where the context decides:

```lisette
// !callout-right supplied
let scores = Map.new<string, int>()
// !callout-right inferred as `Slice<string>`
let names = ["Alice", "Bob"]
```

### Bounds

A type parameter can carry a bound `T: Constraint` that constrains which types may instantiate it. Any [interface](/docs/interfaces/) serves as a constraint, and the prelude [supplies](/docs/prelude/constraints/) two: `Comparable` and `Ordered`.

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

