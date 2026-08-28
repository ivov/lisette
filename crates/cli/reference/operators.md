---
title: "Operators"
description: "Precedence, value, access, expression and assignment operators"
---

## Precedence

Precedence decides how an expression groups when it mixes operators, from lowest to highest:

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

`+`, `-`, `*`, `/`, `%` require both operands to be the same numeric type. An untyped numeric literal adapts to the other operand, and unary `-` negates a number.

`+` also concatenates strings:

```lisette
let greeting = "hello" + ", " + "world"
```

### Comparison

`==` and `!=` compare two values of the same [comparable](/docs/prelude/constraints/#comparable) type. `<`, `>`, `<=`, `>=` compare numeric types and strings. All of them return `bool`.

```lisette
// !callout-right `true`
let same = 2 == 2
// !callout-right `true`, compared byte by byte
let ordered = "apple" < "banana"
```

### Logical

`&&` and `||` short-circuit: the right operand is not evaluated if the left determines the result. `!` negates. All require `bool` operands.

```lisette
if is_valid && count > 0 {
  process()
}
```

### Bitwise

`&`, `|`, `^`, and `&^` operate on integer values. Shifts (`<<`, `>>`) require an integer left operand and any integer right operand. The result has the left operand's type.

```lisette
let mask = 0b1111
let value = 0b1010

let masked = value & mask
let toggled = value ^ mask
let shifted = value << 2
let inverted = ^value
```

## Access operators

### Indexed access

Arrays and slices support integer indexing:

```lisette
let nums = [10, 20, 30]
// !callout-right `10`
let first = nums[0]

let fixed: Array<int, 3> = [10, 20, 30]
// !callout-right `20`
let second = fixed[1]
```

Maps support key indexing:

```lisette
let mut ages = Map.new<string, int>()
ages["Alice"] = 20
// !callout-right `20`
let age = ages["Alice"]
```

Bracket access on an array or slice panics if the index is out of bounds. Bracket access on maps returns the zero value if the key is missing. If the map's value type has no zero value (e.g. `Ref<T>`), bracket reads are rejected at compile time.

For access that cannot panic, use [`.get()`](/docs/prelude/slice/#sliceget), which returns an `Option<T>`.

Range indexing is available only on slices. To range-index an array, first copy its elements into a slice:

```lisette
let tail = fixed.to_slice()[1..]
```

### Range

| Syntax        | Type                  | Description               |
| ------------- | --------------------- | ------------------------- |
| `start..end`  | `Range<T>`            | Exclusive upper bound     |
| `start..=end` | `RangeInclusive<T>`   | Inclusive upper bound     |
| `start..`     | `RangeFrom<T>`        | No upper bound            |
| `..end`       | `RangeTo<T>`          | Exclusive, no lower bound |
| `..=end`      | `RangeToInclusive<T>` | Inclusive, no lower bound |

The `..` and `..=` operators build these values, which drive a [`for` loop](/docs/control-flow/#for) or index a slice:

```lisette
// !callout-right elements at indices `1, 2, 3`
let slice = items[1..4]
```

Slice sub-slicing is [safe by default](/docs/intro/safety/#sub-slicing). The resulting sub-slice has its capacity capped to its length, so `append` on a sub-slice always allocates a new backing array and never silently mutates the original.

### Ref and deref

`&expr` points at a value instead of copying it, giving a [`Ref<T>`](/docs/references/) that several places can hold at once. `ref.*` dereferences it, reading back the value it points at.

```lisette
let x = 42
let r = &x
// !callout-right `42`
let value = r.*
```

## Expression operators

### Pipeline

The pipeline operator `|>` passes the left side as the first argument to the function on the right.

```lisette
// !callout-right equivalent to `f(x)`
x |> f
// !callout-right equivalent to `f(x, y)`
x |> f(y)
// !callout-right equivalent to `f(x, y, z)`
x |> f(y, z)
```

Chains read top to bottom:

```lisette
let result = items
  |> filter(is_valid)
  |> map(transform)
// !callout[/sum/] equivalent to `sum(map(filter(items, is_valid), transform))`
  |> sum()
```

Lambdas are not allowed as pipeline targets.

### Error propagation

The `?` operator [propagates](/docs/failures/) a failure.

### Type conversion

The `as` operator [converts](/docs/types/#type-conversion) between numeric types.

## Assignment operators

### Simple assignment

`=` assigns to a mutable target.

```lisette
let mut name = "Alice"
name = "Bob"
```

### Compound assignment

`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `&^=`, `<<=`, `>>=` combine an operation with an assignment.

```lisette
let mut count = 0
count += 1

let mut total = 10.0
total *= 1.5
```
