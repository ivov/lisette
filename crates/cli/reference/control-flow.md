---
title: "Control flow"
description: "Blocks, if else, for, while, loop, break, continue, return, defer"
---

## Blocks

A block is a sequence of expressions inside braces. The last expression is the block's value. Bindings inside a block are not visible outside of it.

```lisette
let value = {
  let a = 1
  let b = 2
  // !callout-right `value` is `3`
  a + b
}
```

## `if else`

`if` runs a block when a condition is `true`. An optional `else` runs a block when the condition is `false`.

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

With an `else`, `if` returns the value of the branch that runs. All branches must have the same type.

```lisette
let score = get_score()

let result = if score >= 50 { "pass" } else { "fail" }

let grade = if score >= 90 {
  "A"
} else if score >= 80 {
  "B"
} else {
  "C"
}
```

## `if let`

Runs the body when a [pattern](/docs/pattern-matching/) matches. Typically used with `Option`.

```lisette
let mut ages = Map.new<string, int>()
ages["Alice"] = 30

if let Some(age) = ages.get("Alice") {
  fmt.Println(f"Alice is {age}")
}
```

With an `else`, `if let` returns the value of the branch that runs:

```lisette
let message = if let Some(age) = ages.get("Bob") {
  f"Bob is {age}"
} else {
  "Bob's age is unknown"
}
```

## `for`

Iterates over a collection or range.

```lisette
// !callout-above[/item/] immutable copy of each element
for item in items {
  fmt.Println(item)
}
// !callout-above[/item/] mutable copy of each element
for mut item in items {
  item *= 2
  fmt.Println(item)
}
```

Writing to a mutable copy does not affect the original. To write to the original in the collection, write through the index.

```lisette
for mut item in items {
  // !callout-right does not change `items`
  item *= 2
  fmt.Println(item)
}

for i in 0..items.length() {
  // !callout-right changes `items`
  items[i] *= 2
}
```

`for mut` makes a `Slice`, `Map`, or `Ref` element writable, so a write through it changes the original.

```lisette
let mut rows = [[1, 2], [3, 4]]
// !callout-above[/row/] `mut Slice<int>`
for mut row in rows {
  // !callout-right changes `rows`
  row[0] = 99
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

A loop over a `Map` must bind each entry as a `(key, value)` pair:

```lisette
for (name, age) in ages {
  fmt.Println(name, age)
}
```

[`enumerate()`](/docs/prelude/slice/#sliceenumerate) pairs each element of a `Slice` with its index:

```lisette
for (i, item) in items.enumerate() {
  fmt.Println(i, item)
}
```

## `while`

Repeats while a condition is `true`.

```lisette
let mut total = 0
while total < 10 {
  total += 3
}
```

## `while let`

Repeats as many times as a pattern matches.

```lisette
let mut i = 0
while let Some(item) = items.get(i) {
  fmt.Println(item)
  i += 1
}
```

## `loop`

Loops forever. Exit with `break`.

```lisette
let mut n = 0
loop {
  n += 1
  if n == 10 {
    break
  }
}
```

`break` can carry a value, which becomes the value of the `loop`.

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

Schedules an expression to run whenever the enclosing function returns.

```lisette
fn file_size(path: string) -> Result<int64, error> {
  let file = os.Open(path)?
  // !callout-right runs whether `file.Stat()` succeeds or fails
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

