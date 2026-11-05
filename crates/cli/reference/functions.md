---
title: "Functions"
description: "Definitions, generic functions, write permission in parameters, lambdas, iterators"
---

A function has a name, parameters with type annotations, an optional return type, and a body.

```lisette
fn add(a: int, b: int) -> int {
  a + b
}

fn greet(name: string) {
  fmt.Println(f"Hello, {name}")
}
```

Parameter types are required. The return type can be omitted, in which case it defaults to `()`.

The last expression in the function body is the return value. Use `return` for early exits.

```lisette
fn first_positive(nums: Slice<int>) -> Option<int> {
  for n in nums {
    if n > 0 {
      // !callout-right early exit
      return Some(n)
    }
  }

  // !callout-right returned automatically
  None
}
```

## Generic functions

Type parameters appear in angle brackets after the function name.

```lisette
fn first<T>(xs: Slice<T>) -> Option<T> {
  xs.get(0)
}

fn swap<A, B>(pair: (A, B)) -> (B, A) {
  (pair.1, pair.0)
}
```

The compiler infers type arguments at call sites:

```lisette
// !callout-right `T` is `int`
let n = first([1, 2, 3])
// !callout-right `T` is `string`
let s = first(["a", "b"])
// !callout-right `A` is `int`, `B` is `string`
let swapped = swap((1, "one"))
```

Type arguments are required when inference has nothing to work with:

```lisette
// !callout-right no elements to infer `T` from
let nums = Slice.new<int>()
// !callout-right no entries to infer `K` and `V` from
let counts = Map.new<string, int>()
```

## Write permission in parameters

A parameter's type carries write permission, signaling whether it can write through to the caller's data.

```lisette
// !callout-right cannot write to `items`
fn total(items: Slice<int>) -> int
// !callout-right can write to `items`
fn sort(items: mut Slice<int>)
```

A writable value fits a read-only parameter, but never the reverse.

```lisette
let mut scores = [3, 1, 2]
let frozen = [3, 1, 2]

// !callout-ok-right writable value, read-only parameter
let sum = total(scores)
// !callout-ok-right writable value, writable parameter
sort(scores)
// !callout-error-right error: `frozen` was declared without `mut`
sort(frozen)
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
let timestamp = || time.Now().Unix()
```

Lambda parameter types can be omitted when inferable:

```lisette
fn passing(scores: Slice<int>) -> Slice<int> {
  // !callout[/(?<=\|)s(?=\|)/] `s` is `int`
  scores.filter(|s| s >= 50)
}
```

A block body allows multiple statements:

```lisette
let describe = |n: int| {
  let kind = if n % 2 == 0 { "even" } else { "odd" }
  f"{n} is {kind}"
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
