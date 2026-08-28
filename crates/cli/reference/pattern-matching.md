---
title: "Pattern matching"
description: "match, exhaustiveness, patterns, alternative patterns, as bindings, guards, let else"
---

A pattern tests a value's shape and names the parts inside it.

## Match expressions

`match` runs the first arm whose pattern fits the value, and evaluates to what that arm returns.

```lisette
fn describe(n: int) -> string {
  match n {
    0 => "zero",
    1 => "one",
    _ => "many",
  }
}
```

## Exhaustiveness

Patterns must cover all possible values:

```lisette
enum Color { Red, Green, Blue }

match color {
  Color.Red => "red",
  Color.Green => "green",
}
```

The compiler enforces exhaustiveness:

```ansi wrap
  [31m✕[0m [95mmatch[39m[1m[31m is not exhaustive[39m[0m
   ╭─[example.lis:4:3]
 [2m3[0m │ fn name(color: Color) -> string {
 [2m4[0m │   [31mmatch color[0m {
   · [31m  ─────┬─────[0m
   ·        [31m╰── [31m[31mnot all patterns covered[39m[0m[0m
 [2m5[0m │     Color.Red => "red",
   ╰────
[2m  help: [0m[2mHandle the missing case [0m[95mColor.Blue[39m[2m, e.g. [0m[95mColor.Blue => { ... }[39m[2m · code: [0m[2m[infer.non_exhaustive][0m
```

## Matchable patterns

### Literals

Integer, boolean, string, and rune literals match exact values:

```lisette
match status {
  "ok" => 200,
  "missing" => 404,
  _ => 500,
}
```

```lisette
match letter {
  'a' => "letter a",
  'b' => "letter b",
  _ => "other",
}
```

### Bindings

An identifier binds the matched value to a name:

```lisette
match opt {
  // !callout-right `n` is the value inside `Some`
  Some(n) => n * 2,
  None => 0,
}
```

`_` matches any value without binding:

```lisette
match opt {
  // !callout-right `_` discards the value inside `Some`
  Some(_) => "has value",
  None => "empty",
}
```

### Tuples

Tuple patterns destructure by position:

```lisette
let pair = (10, 20)

match pair {
  (0, 0) => "origin",
  (x, 0) => f"on x-axis at {x}",
  (0, y) => f"on y-axis at {y}",
  (x, y) => f"at ({x}, {y})",
}
```

### Structs

Struct patterns match fields by name:

```lisette
struct Point {
  x: int,
  y: int,
}

let point = Point { x: 10, y: 0 }

match point {
  Point { x: 0, y: 0 } => "origin",
  Point { x, y: 0 } => f"on x-axis at {x}",
  Point { x, y } => f"at ({x}, {y})",
}
```

Use `..` to ignore remaining fields:

```lisette
struct User {
  name: string,
  email: string,
  age: int,
}

let user = User {
  name: "Alice",
  email: "alice@example.com",
  age: 30,
}

match user {
  User { name: "", .. } => "hello, stranger",
  // !callout[/\.\./] `..` ignores `email` and `age`
  User { name, .. } => f"hello, {name}",
}
```

### Enum variants

Enum patterns match variants and destructure their payloads:

```lisette
enum Message {
  Ready,
  Write(string),
  Move { x: int, y: int },
}

let msg = Message.Write("hello")

match msg {
  Message.Ready => "ready",
  Message.Write(text) => f"writing: {text}",
  Message.Move { x, y } => f"moving to ({x}, {y})",
}
```

Inside a `match` arm, the enum qualifier can be omitted:

```lisette
match msg {
  // !callout `Message.` qualifier omitted
  Ready => "ready",
  Write(text) => f"writing: {text}",
  Move { x, y } => f"moving to ({x}, {y})",
}
```

### Slices and arrays

Bracketed patterns match slice and array elements:

```lisette
let items = [1, 2, 3]

match items {
  [] => "empty",
  [n] => f"single: {n}",
  [first, second] => f"pair: {first}, {second}",
  // !callout[/\.\.rest/] `rest` is `[2, 3]`
  [first, ..rest] => f"first is {first}, {rest.length()} more",
}
```

The rest pattern `..rest` captures remaining elements as a `Slice` when matching a slice, or as an `Array` when matching an array. It must appear last. Elements after `..` are not allowed.

Use `..` without an identifier to ignore the rest:

```lisette
match items {
  // !callout[/\.\./] `..` discards all remaining elements
  [first, ..] => first,
  [] => 0,
}
```

## Alternative patterns

Use `|` to match multiple patterns in one arm:

```lisette
match direction {
  North | South => "north-south",
  East | West => "east-west",
}
```

Alternatives can bind variables if all of them bind the same names:

```lisette
enum Event {
  KeyDown(rune),
  KeyUp(rune),
}

match event {
  // !callout[/(?<=\{)key/] `key` is `rune`
  KeyDown(key) | KeyUp(key) => f"key: {key}",
}
```

## `as` for value capture

Use `as` to capture the entire matched value:

```lisette
let mut history = Slice.new<Message>()

match msg {
  Ready => "ready",
  Write(text) => text,
  // !callout[/(?<=as )moved/] `moved` is `Move` itself
  Move { x, .. } as moved => {
    history = history.append(moved)
    f"moved to {x}"
  }
}
```

For an arm with alternatives, place `as` on each one:

```lisette
match event {
  KeyDown(key) as pressed | KeyUp(key) as pressed => record(pressed, key),
}
```

## Pattern guards

Add `if` after a pattern to require an additional condition:

```lisette
match opt {
  Some(n) if n > 0 => "positive",
  Some(_) => "non-positive",
  None => "empty",
}
```

A guard can use a value captured with `as`:

```lisette
match opt {
  Some(Point { x, .. }) as point if x > 0 => transform(point),
  _ => default,
}
```

Guards do not count toward exhaustiveness. If all arms have guards, a wildcard or catch-all arm is still required.

## `let else`

A plain [`let`](/docs/bindings/#destructuring) needs a pattern that always matches. `Some(n)` might not, so use `let else`. The `else` branch runs when the match fails, and must `return`, `break`, or `continue`.

```lisette
fn double_or_zero(opt: Option<int>) -> int {
  let Some(n) = opt else { return 0 }
  n * 2
}
```

A slice pattern that constrains the length can fail, because a slice has no fixed length:

```lisette
fn first_two(items: Slice<int>) -> int {
  let [first, second, ..] = items else { return 0 }
  first + second
}
```

