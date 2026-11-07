---
title: "Operators"
description: "Precedence, value, access, expression and assignment operators"
---

## Precedence

Precedence decides how an expression groups when it mixes operators. From lowest to highest:

| Precedence | Operators                      | Description                                     |
| ---------- | ------------------------------ | ----------------------------------------------- |
| 1          | `\|>`                          | Pipeline                                        |
| 2          | `\|\|`                         | Logical or                                      |
| 3          | `&&`                           | Logical and                                     |
| 4          | `==` `!=` `<` `>` `<=` `>=`    | Comparison                                      |
| 5          | `..` `..=`                     | Range                                           |
| 6          | `+` `-` `\|` `^`               | Add/subtract, bitwise or/xor                    |
| 7          | `*` `/` `%` `<<` `>>` `&` `&^` | Multiply/divide, shifts, bitwise and/and-not    |
| 8          | `as`                           | Type conversion                                 |
| 9          | `-` `!` `^` `&`                | Prefix (negation, not, bitwise not, reference)  |
| 10         | `.` `()` `[]` `?` `.*`         | Postfix (access, call, index, propagate, deref) |

All binary operators are left-associative, so a repeated operator groups from the left.

```lisette
// !callout-right parses as `a + (b * c)`
a + b * c
// !callout-right parses as `(a && b) || c`
a && b || c
// !callout-right parses as `f(a + b)`
a + b |> f
// !callout-right parses as `0..(1 + 2)`
0..1 + 2
```

## Value operators

### Arithmetic

`+`, `*`, `/`, `%` require both operands to be the same numeric type. Unary `-` negates a number. An untyped numeric literal adapts to the other operand.

```lisette
let size: int64 = 4
let ratio: float64 = 1.5
// !callout[/next/] `int64`
let next = size + 1
// !callout[/double/] `float64`
let double = ratio * 2
```

`+` also concatenates strings:

```lisette
// !callout-right `"hello, world"`
"hello" + ", " + "world"
```

### Comparison

`==` and `!=` compare two values of the same [comparable](/docs/prelude/constraints/#comparable) type. `<`, `>`, `<=`, `>=` compare numeric types and strings. All of them return `bool`.

```lisette
// !callout-right `true`
2 == 2
// !callout-right `true`, compared byte by byte
"apple" < "banana"
```

### Logical

`&&` is `true` when both operands are `true`, `||` is `true` when at least one is, and `!` negates. All require `bool` operands. `&&` and `||` short-circuit, i.e. if the left operand already determines the result, the right operand is not evaluated.

```lisette
let items = [3, 0, 7]
let i = 5
// !callout[/</] `false`, so `items[i] > 0` is skipped
if i < items.length() && items[i] > 0 {
  fmt.Println(items[i])
}
```

### Bitwise

`&` (and), `|` (or), `^` (xor), and `&^` (and-not) combine two integers bit by bit. Both operands must have the same integer type. Unary `^` flips every bit. `<<` and `>>` shift the left operand by the right operand, which can be any integer type. The result has the left operand's type.

```lisette
let value = 0b1010
// !callout-right `0b0010`
value & 0b0110
// !callout-right `0b1111`
value | 0b0101
// !callout-right `0b1100`
value ^ 0b0110
// !callout-right `0b1000`
value &^ 0b0010
// !callout-right `0b101000`
value << 2
// !callout-right `0b0101`
value >> 1
// !callout-right `-11`
^value
```

## Access operators

### Indexing

Slices and arrays support integer indexing:

```lisette
let scores = [90, 85, 77]
let rgb: Array<int, 3> = [255, 128, 0]

// !callout-right `90`
scores[0]
// !callout-right `128`
rgb[1]
```

Maps support key indexing:

```lisette
let mut ages = Map.new<string, int>()
ages["Alice"] = 20

// !callout-right `20`
ages["Alice"]
```

Indexing a slice or array panics if the index is out of bounds. Indexing a map returns the zero value if the key is missing. For access that cannot panic, use [`.get()`](/docs/prelude/slice/#sliceget), which returns an `Option<T>`.

```lisette
let scores = [90, 85, 77]
let ages = Map.new<string, int>()

// !callout-error-right panic: index out of range
scores[5]
// !callout-right `0`
ages["Bob"]

// !callout-right `None`
scores.get(5)
```

### Range

A range is a span of values from a start to an end. One bound can be left out.

| Syntax        | Type                  | Description               |
| ------------- | --------------------- | ------------------------- |
| `start..end`  | `Range<T>`            | Exclusive upper bound     |
| `start..=end` | `RangeInclusive<T>`   | Inclusive upper bound     |
| `start..`     | `RangeFrom<T>`        | No upper bound            |
| `..end`       | `RangeTo<T>`          | Exclusive, no lower bound |
| `..=end`      | `RangeToInclusive<T>` | Inclusive, no lower bound |

A range can drive a [`for` loop](/docs/control-flow/#for) or index a slice:

```lisette
// !callout-right elements at indices `1, 2, 3`
let slice = items[1..4]
```

Slice sub-slicing is [safe by default](/docs/intro/safety/#sub-slicing). The resulting sub-slice has its capacity capped to its length, so `append` on a sub-slice always allocates a new backing array and never silently mutates the original.

### Ref and deref

`&` creates a [`Ref<T>`](/docs/references/) that points at a value instead of copying it. `.*` reads the value back.

```lisette
let count = 42
// !callout-right `Ref<int>`
let ref = &count

// !callout-right `42`
let value = ref.*
```

## Expression operators

### Pipeline

`|>` passes the value on its left as the first argument of the function on its right.

```lisette
// !callout-right same as `f(x)`
x |> f
// !callout-right same as `f(x, y)`
x |> f(y)
// !callout-right same as `f(x, y, z)`
x |> f(y, z)
```

Chains read top to bottom:

```lisette
let result = items
  |> filter(is_valid)
  |> map(transform)
// !callout[/sum/] same as `sum(map(filter(items, is_valid), transform))`
  |> sum()
```

Lambdas are not allowed as pipeline targets.

### Error propagation

The `?` operator [propagates](/docs/failures/) a failure.

### Type conversion

The `as` operator [converts](/docs/types/#type-conversion) between numeric types.

## Assignment operators

`=` assigns to a mutable target.

```lisette
let mut name = "Alice"
name = "Bob"
```

`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `&^=`, `<<=`, `>>=` combine an operation with an assignment.

```lisette
let mut count = 0
let mut total = 10

// !callout-right `count` is `1`
count += 1
// !callout-right `total` is `20`
total *= 2
```
