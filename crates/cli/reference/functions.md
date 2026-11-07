---
title: "Functions"
description: "Definitions, generic functions, write permission in parameters, lambdas, iterators"
---

A function has a name, parameters with type annotations, an optional return type, and a body.

```lisette
fn to_fahrenheit(celsius: float64) -> float64 {
  celsius * 9.0 / 5.0 + 32.0
}
```

Parameter types are required. The return type can be omitted, in which case it defaults to `()`.

The last expression in the function body is the return value. Use `return` for early exits.

```lisette
fn first_positive(nums: Slice<int>) -> Option<int> {
  for n in nums {
    if n > 0 {
      // !callout-right early return value
      return Some(n)
    }
  }

  // !callout-right return value
  None
}
```

## Generic functions

A generic function features [type parameters](/docs/types/#type-parameters) in its signature:

```lisette
// !callout-center[/(?<=pick_random<)T/] declared
// !callout-center[/(?<=Slice<)T/] used
// !callout-center[/(?<=Option<)T/] used
fn pick_random<T>(items: Slice<T>) -> Option<T> {
  if items.is_empty() {
    return None
  }
  items.get(rand.IntN(items.length()))
}
```

```lisette
fn swap<A, B>(pair: (A, B)) -> (B, A) {
  (pair.1, pair.0)
}
```

At each callsite, the arguments fill in the type parameters:

```lisette
// !callout-right `T` is `int`
let n = pick_random([1, 2, 3])
// !callout-right `T` is `string`
let s = pick_random(["a", "b"])
```

```lisette
// !callout-right `A` is `int`, `B` is `string`
let swapped = swap((1, "one"))
```

## Write permission in parameters

Only a `mut` parameter can write to the caller's data:

```lisette
// !callout-right `total` cannot write to `items`
fn total(items: Slice<int>) -> int
// !callout-right `sort` can write to `items`
fn sort(items: mut Slice<int>)
```

A writable value can be passed to a read-only parameter, but a read-only value cannot be passed to a writable one.

```lisette
let mut scores = [72, 95, 88]
let prices = [12, 5, 30]

// !callout-ok-right writable value, read-only parameter
let sum = total(scores)
// !callout-ok-right writable value, writable parameter
sort(scores)
// !callout-error-right error: `prices` was declared without `mut`
sort(prices)
```

Parameters are read-only. To change one, copy it into a `let mut` binding with the same name.

```lisette
fn digits(n: int) -> int {
  // !callout-right mutable copy of `n`
  let mut n = n
  let mut count = 1
  while n >= 10 {
    n /= 10
    count += 1
  }
  count
}
```

## Lambdas

Lambdas are anonymous inline functions.

```lisette
let is_even = |n: int| n % 2 == 0
let full_name = |first: string, last: string| f"{first} {last}"
let timer = time.AfterFunc(5 * time.Second, || fmt.Println("timed out"))
```

A block body allows multiple statements:

```lisette
let describe = |n: int| {
  let kind = if n % 2 == 0 { "even" } else { "odd" }
  f"{n} is {kind}"
}
```

A lambda can omit a parameter type when the context sets it:

```lisette
fn passing(scores: Slice<int>) -> Slice<int> {
  // !callout-center[/(?<=\|)s(?=\|)/] `int`
  scores.filter(|s| s >= 50)
}
```

Lambdas can capture variables from the enclosing scope. A write inside the lambda changes the outer variable:

```lisette
let mut calls = 0
let log = |message: string| {
  fmt.Println(message)
  calls += 1
}

log("start")
log("stop")
// !callout-right `2`
calls
```

## Iterators

A `for` loop accepts any Go [`iter.Seq<T>`](https://pkg.go.dev/iter#Seq) or [`iter.Seq2<K, V>`](https://pkg.go.dev/iter#Seq2) value. To write one, return a lambda that takes a `yield` function. Call `yield` once per element, and return as soon as `yield` gives back `false`:

```lisette
import "go:iter"

fn count_up(n: int) -> iter.Seq<int> {
  |yield: fn(int) -> bool| {
    for i in 0..n {
      if !yield(i) { return }
    }
  }
}
```

```lisette
for i in count_up(1000) {
  // !callout-right ends the iterator
  if i == 3 { break }
  fmt.Println(i)
}
```
