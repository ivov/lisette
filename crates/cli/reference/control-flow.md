---
title: "Control flow"
description: "Blocks, if else, for, while, loop, break, continue, return, defer"
---

## Blocks

A block is a sequence of expressions inside braces `{ ... }`. The last expression is the block's value. Bindings inside are not visible outside.

```lisette
let value = {
  let a = 10
  let b = 20
  // !callout-right `value` is `30`
  a + b
}
```

## `if else`

```lisette
if count > 0 {
  fmt.Println("has items")
}

if count > 10 {
  fmt.Println("large")
} else {
  fmt.Println("small")
}
```

When both branches are present, `if else` returns a value. Both branches must produce the same type.

```lisette
let label = if count > 10 { "large" } else { "small" }

let clamped = if n > max {
  max
} else if n < min {
  min
} else {
  n
}
```

An `if` without `else` has type `()`.

### `if let`

Runs the body when a [pattern](/docs/pattern-matching/) matches. Most commonly used with `Option`.

```lisette
if let Some(x) = opt {
  fmt.Println(x)
}
```

With `else`, it works as an expression:

```lisette
let value = if let Some(x) = opt {
  x
} else {
  0
}
```

## `for`

Iterates over a collection or range, for its side effects.

```lisette
for item in items {
  fmt.Println(item)
}

for i in 0..5 {
  // !callout-right prints `0, 1, 2, 3, 4`
  fmt.Println(i)
}
```

Supported iterables:

| Iterable                                        | Element type |
| ----------------------------------------------- | ------------ |
| `Slice<T>`                                      | `T`          |
| `Array<T, N>`                                   | `T`          |
| `Map<K, V>`                                     | `(K, V)`     |
| `Range<T>`, `RangeInclusive<T>`, `RangeFrom<T>` | `T`          |
| `Channel<T>`, `Receiver<T>`                     | `T`          |
| `EnumeratedSlice<T>`                            | `(int, T)`   |
| `iter.Seq<T>`                                   | `T`          |
| `iter.Seq2<K, V>`                               | `(K, V)`     |

📚 See [Functions](/docs/functions/#iterators) for writing an iterator

To iterate over a [string](/docs/prelude/strings/), pick a unit:

```lisette
for r in s.runes() {
  fmt.Println(r)
}

for b in s.bytes() {
  fmt.Println(b)
}
```

Maps require destructuring into key and value:

```lisette
for (name, age) in ages {
  fmt.Println(name, age)
}
```

Use `enumerate()` for indexed iteration over a slice:

```lisette
for (i, item) in items.enumerate() {
  fmt.Println(i, item)
}
```

Open-ended ranges `start..` loop until a `break`:

```lisette
for i in 0.. {
  if i >= 10 {
    break
  }
}
```

## `while`

Repeats while a condition is `true`, for its side effects.

```lisette
let mut total = 0
while total < 10 {
  total += 3
}
```

### `while let`

Repeats as many times as a pattern matches.

```lisette
let mut i = 0
while let Some(item) = items.get(i) {
  fmt.Println(item)
  i += 1
}
```

## `loop`

An infinite loop. Exit with `break`.

```lisette
let mut n = 0
loop {
  n += 1
  if n == 10 {
    break
  }
}
```

A `loop` has no exit other than `break`, which can carry a value, so a `loop` evaluates to the value carried by `break`.

```lisette
let mut n = 0
let result = loop {
  n += 7
  if n > 20 {
    // !callout-right `result` is `21`
    break n
  }
}
```

A bare `break` gives the loop type `()`.

## `break` and `continue`

`break` exits a loop. `continue` skips to the next iteration of the loop.

```lisette
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

```lisette
fn find(items: Slice<int>, target: int) -> Option<int> {
  for i in 0..items.length() {
    if items[i] == target {
      // !callout-right early return
      return Some(i)
    }
  }
  None
}
```

## `defer`

Schedules an expression to run when the enclosing function returns, no matter how it returns.

```lisette
fn file_size(path: string) -> Result<int64, error> {
  let file = os.Open(path)?
  // !callout-right runs on function exit, whether `file.Stat()` succeeds or fails
  defer file.Close()
  let info = file.Stat()?
  Ok(info.Size())
}
```

When a function has several `defer` calls, the last one scheduled runs first.

```lisette
defer fmt.Println("first")
// !callout-right runs before `first`
defer fmt.Println("second")
```

A `defer` block groups cleanup steps that belong together. Inside one block the steps run in the order written, top to bottom.

```lisette
defer {
  // !callout-right runs first
  conn.flush()
  // !callout-right runs second
  conn.close()
}
```

