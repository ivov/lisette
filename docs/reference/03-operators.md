# Operators

## Precedence

From lowest to highest:

| Precedence | Operators                   | Description                                     |
| ---------- | --------------------------- | ----------------------------------------------- |
| 1          | `\|>`                       | Pipeline                                        |
| 2          | `\|\|`                      | Logical or                                      |
| 3          | `&&`                        | Logical and                                     |
| 4          | `==` `!=` `<` `>` `<=` `>=` | Comparison                                      |
| 5          | `..` `..=`                  | Range                                           |
| 6          | `+` `-`                     | Addition, subtraction                           |
| 7          | `*` `/` `%`                 | Multiplication, division                        |
| 8          | `as`                        | Type cast                                       |
| 9          | `-` `!` `&`                 | Prefix (negation, not, reference)               |
| 10         | `.` `()` `[]` `?` `.*`      | Postfix (access, call, index, propagate, deref) |

All binary operators are left-associative.

```rust
a + b * c          // a + (b * c)
a && b || c        // (a && b) || c
a + b |> f         // f(a + b)
0..1 + 2           // 0..(1 + 2)
```

## Arithmetic

`+`, `-`, `*`, `/`, `%` require both operands to be the same numeric type. An exception is made for named numeric types from Go (like `time.Duration`), which can be used in arithmetic with their underlying type. See [`13-go-interop.md`](13-go-interop.md)

Unary `-` negates a number. Disallowed for unsigned types:

```
  [error] Cannot negate unsigned type
   ╭─[example.lis:1:16]
 1 │ let x: uint8 = -1
   ·                ─┬
   ·                 ╰── cannot negate `uint8`
   ╰────
  help: Unsigned types cannot represent negative values
```

`+` also concatenates strings:

```rust
let greeting = "hello" + ", " + "world"
```


## Comparison

`==` and `!=` compare any two values of the same type.

`<`, `>`, `<=`, `>=` compare numeric types and strings.

All comparison operators return `bool`.

## Logical

`&&` and `||` short-circuit: the right operand is not evaluated if the left determines the result. `!` negates. All require `bool` operands.

```rust
if is_valid && count > 0 {
  process()
}
```

## Pipeline

The pipeline operator `|>` passes the left side as the first argument to the function on the right.

```rust
x |> f              // f(x)
x |> f(y)           // f(x, y)
x |> f(y, z)        // f(x, y, z)
```

Chains read top to bottom:

```rust
let result = items
  |> filter(is_valid)
  |> map(transform)
  |> sum()

// equivalent to: sum(map(filter(items, is_valid), transform))
```

The right side must be a function call. Lambdas are not allowed as pipeline targets.

## Range

Range operators `..` and `..=` create range values.

| Syntax        | Type                  | Description               |
| ------------- | --------------------- | ------------------------- |
| `start..end`  | `Range<T>`            | Exclusive upper bound     |
| `start..=end` | `RangeInclusive<T>`   | Inclusive upper bound     |
| `start..`     | `RangeFrom<T>`        | No upper bound            |
| `..end`       | `RangeTo<T>`          | Exclusive, no lower bound |
| `..=end`      | `RangeToInclusive<T>` | Inclusive, no lower bound |

Ranges are used in `for` loops and slice indexing:

```rust
for i in 0..5 {
  fmt.Println(i)              // 0, 1, 2, 3, 4
}

for i in 0..=5 {
  fmt.Println(i)              // 0, 1, 2, 3, 4, 5
}

let slice = items[1..4]       // elements at indices 1, 2, 3
```

Slice sub-slicing is safe by default. The resulting sub-slice has its capacity capped to its length, so `append` on a sub-slice always allocates a new backing array and never silently mutates the original. 

🧿 See [`safety.md`](../intro/safety.md)

## Reference and dereference

`&expr` creates a `Ref<T>`. `ref.*` dereferences it.

```rust
let x = 42
let r = &x
let value = r.*              // 42
```

📚 See [`07-pointers.md`](07-pointers.md)

## Indexed access

Slices and maps support bracket indexing:

```rust
let nums = [10, 20, 30]
let first = nums[0]          // 10

let mut ages = Map.new<string, int>()
ages["Alice"] = 20
let age = ages["Alice"]      // 20
```

Bracket access panics if the index is out of bounds (slices) or returns the zero value if the key is missing (maps). For safe access that returns `Option<T>`, use `.get()`. See [`safety.md`](../intro/safety.md)

```rust
let safe = nums.get(0)       // Some(10)
let missing = nums.get(99)   // None
```

## Compound assignment

`+=`, `-=`, `*=`, `/=`, `%=` combine an operation with an assignment. The target must be mutable:

```rust
let mut count = 0
count += 1

let mut total = 10.0
total *= 1.5
```

Mutation through a `Ref<T>` does not require `mut` on the binding.

📚 See [`07-pointers.md`](07-pointers.md)

## Error propagation

The `?` operator propagates errors from `Result` and `None` from `Option`.

📚 See [`09-error-handling.md`](09-error-handling.md)

## Type cast

The `as` operator converts between numeric types and between `string` and `Slice<byte>` or `Slice<rune>`.

📚 See [`02-types.md`](02-types.md)

<br>

<table><tr>
<td>← <a href="02-types.md"><code>02-types.md</code></a></td>
<td align="right"><a href="04-control-flow.md"><code>04-control-flow.md</code></a> →</td>
</tr></table>
