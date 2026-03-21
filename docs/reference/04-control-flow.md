# Control Flow

Lisette is expression-oriented: blocks, `if`/`else`, and `loop` produce values. `for` and `while` are statements, so they do not produce values. Branching with `match` is covered in [`08-pattern-matching.md`](08-pattern-matching.md).

## Blocks

A block is a sequence of expressions in braces. The last expression is the block's value. Bindings inside are not visible outside.

```rust
let value = {
  let a = 10
  let b = 20
  a + b
}
// value == 30
```

## `if` / `else`

```rust
if count > 0 {
  process()
}

if count > 10 {
  fmt.Println("large")
} else {
  fmt.Println("small")
}
```

When both branches are present, `if`/`else` returns a value. Both branches must produce the same type.

```rust
let label = if count > 10 { "large" } else { "small" }

let clamped = if x > max {
  max
} else if x < min {
  min
} else {
  x
}
```

An `if` without `else` has type `()`.

## `if let`

Runs the body when a pattern matches. Most commonly used with `Option`.

```rust
if let Some(x) = opt {
  fmt.Println(x)
}
```

With `else`, it works as an expression:

```rust
let value = if let Some(x) = opt {
  x
} else {
  0
}
```

📚 See [`08-pattern-matching.md`](08-pattern-matching.md)

## `for`

Iterates over a collection or range.

```rust
for item in items {
  process(item)
}

for i in 0..5 {
  fmt.Println(i)            // 0, 1, 2, 3, 4
}

for i in 0..=5 {
  fmt.Println(i)            // 0, 1, 2, 3, 4, 5
}
```

Supported iterables:

| Iterable                                        | Element type |
| ----------------------------------------------- | ------------ |
| `Slice<T>`                                      | `T`          |
| `string`                                        | `rune`       |
| `Map<K, V>`                                     | `(K, V)`     |
| `Range<T>`, `RangeInclusive<T>`, `RangeFrom<T>` | `T`          |
| `Channel<T>`, `Receiver<T>`                     | `T`          |
| `items.enumerate()`                              | `(int, T)`   |

Maps require destructuring into key and value:

```rust
for (name, age) in ages {
  fmt.Println(name, age)
}
```

Use `enumerate()` for indexed iteration over a slice:

```rust
for (i, item) in items.enumerate() {
  fmt.Println(i, item)
}
```

Open-ended ranges `start..` loop until a `break`:

```rust
for i in 0.. {
  if i >= 10 {
    break
  }
}
```

`for` is a statement, so it does not produce a value.

## `while`

Repeats while a condition is true.

```rust
let mut count = 0
while count < 10 {
  count += 1
}
```

### `while let`

Repeats while a pattern matches.

```rust
while let Some(item) = iter.next() {
  process(item)
}
```

`while` is a statement, so it does not produce a value.

## `loop`

An infinite loop. Exit with `break`.

```rust
loop {
  if done() {
    break
  }
}
```

`break` can carry a value, making `loop` an expression. All `break` expressions in a loop must produce the same type. A bare `break` gives the loop type `()`.

```rust
let result = loop {
  let n = try_next()
  if n > 0 {
    break n
  }
}
```

## `break` / `continue`

`break` exits a loop. `continue` skips to the next iteration in the loop.

```rust
for i in 0..100 {
  if i % 2 == 0 {
    continue
  }
  if i > 50 {
    break
  }
  fmt.Println(i)
}
```

There are no labeled breaks. `break` and `continue` always apply to the innermost loop.

## `return`

Returns early from the current function. A bare `return` returns `()`.

```rust
fn find(items: Slice<int>, target: int) -> Option<int> {
  for i in 0..items.length() {
    if items[i] == target {
      return Some(i)
    }
  }
  None
}
```

## `defer`

Schedules an expression to run when the enclosing function returns, regardless of how it returns. Multiple defers execute in reverse order (LIFO).

```rust
fn read_file(path: string) -> Result<Slice<uint8>, error> {
  let file = os.Open(path)?
  defer file.Close()
  io.ReadAll(file)
}
```

A `defer` block groups multiple cleanup steps:

```rust
defer {
  conn.flush()
  conn.close()
}
```

The compiler rejects `defer` in loops, `?` inside defer, and `return`, `break`, or `continue` inside defer.

```
  [error] `defer` inside loop
   ╭─[example.lis:2:3]
 1 │ for i in 0..10 {
 2 │   defer cleanup()
   ·   ───────┬───────
   ·          ╰── not allowed inside loop
 3 │ }
   ╰────
  help: Wrap the loop body in a helper function and call it in the loop
```

📚 See [`safety.md`](../intro/safety.md)

<br>

<table><tr>
<td>← <a href="03-operators.md"><code>03-operators.md</code></a></td>
<td align="right"><a href="05-functions.md"><code>05-functions.md</code></a> →</td>
</tr></table>
