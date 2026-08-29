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
let mut nums = [3, 1, 2]
// !callout-ok-right permission matches
sort(nums)

let nums = [3, 1, 2]
// !callout-error-right error: `nums` was declared without `mut`
sort(nums)
```

Parameters themselves are immutable. To write to one, reassign it with `let mut`.

```lisette
fn digits(n: int) -> int {
  // !callout-right reassignment
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
let double = |x: int| x * 2
let sum = |a: int, b: int| a + b
let produce_int = || 42
```

Lambda parameter types can be omitted when inferable:

```lisette
let doubled = [1, 2, 3].map(|x| x * 2)
```

A block body allows multiple statements:

```lisette
let process = |x: int| {
  let y = x * 2
  y + 1
}
```

Lambdas can capture variables from the enclosing scope:

```lisette
let multiplier = 3
let scale = |x: int| x * multiplier
```

## Iterators

A `for` loop accepts any function returning Go's [`iter.Seq<T>`](https://pkg.go.dev/iter#Seq). To write one, return a lambda that takes a `yield` function. Call `yield` once per element, and return as soon as `yield` gives back `false`:

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
