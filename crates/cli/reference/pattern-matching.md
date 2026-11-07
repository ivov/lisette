---
title: "Pattern matching"
description: "match, exhaustiveness, patterns, or-patterns, as bindings, guards, let else"
---

A pattern tests a value's shape and names the parts inside it.

## Match expressions

`match` checks a value against each arm in order. The first arm that matches runs, and its result becomes the value of the `match`.

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

A `match` must cover every possible value. The compiler rejects a `match` that misses a case:

```lisette
enum Color { Red, Green, Blue }

fn name(color: Color) -> string {
  match color {
    Color.Red => "red",
    Color.Green => "green",
  }
}
```

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
fn http_code(status: string) -> int {
  match status {
    "ok" => 200,
    "missing" => 404,
    _ => 500,
  }
}
```

```lisette
fn answer(key: rune) -> string {
  match key {
    'y' => "yes",
    'n' => "no",
    _ => "unknown",
  }
}
```

### Bindings

An identifier binds the matched value to a name:

```lisette
fn print_age(ages: Map<string, int>, name: string) {
  match ages.get(name) {
    // !callout[/age/] value inside `Some`
    Some(age) => fmt.Println(f"{name} is {age}"),
    None => fmt.Println(f"{name} not found"),
  }
}
```

`_` matches any value without binding it:

```lisette
fn listen(input: string) {
  match strconv.Atoi(input) {
    Ok(port) => fmt.Println(f"listening on {port}"),
    // !callout[/_/] matches any error without binding it
    Err(_) => fmt.Println(f"invalid port: {input}"),
  }
}
```

### Tuples

Tuple patterns destructure by position:

```lisette
fn locate(pair: (int, int)) -> string {
  match pair {
    (0, 0) => "origin",
    (x, 0) => f"on x-axis at {x}",
    (0, y) => f"on y-axis at {y}",
    (x, y) => f"at ({x}, {y})",
  }
}
```

### Structs

Struct patterns match fields by name:

```lisette
struct Point {
  x: int,
  y: int,
}

fn locate(point: Point) -> string {
  match point {
    Point { x: 0, y: 0 } => "origin",
    Point { x, y: 0 } => f"on x-axis at {x}",
    Point { x, y } => f"at ({x}, {y})",
  }
}
```

Use `..` to ignore remaining fields:

```lisette
struct User {
  name: string,
  email: string,
  age: int,
}

fn greet(user: User) -> string {
  match user {
    // !callout[/\.\./] ignores `email` and `age`
    User { name: "", .. } => "hello, stranger",
    _ => f"hello, {user.name}",
  }
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

fn describe(msg: Message) -> string {
  match msg {
    Message.Ready => "ready",
    Message.Write(text) => f"writing: {text}",
    Message.Move { x, y } => f"moving to ({x}, {y})",
  }
}
```

Inside a `match` arm, the enum qualifier can be omitted:

```lisette
fn describe(msg: Message) -> string {
  match msg {
    // !callout short for `Message.Ready`
    Ready => "ready",
    Write(text) => f"writing: {text}",
    Move { x, y } => f"moving to ({x}, {y})",
  }
}
```

### Slices and arrays

Bracketed patterns match slice and array elements:

```lisette
fn summarize(items: Slice<int>) -> string {
  match items {
    [] => "empty",
    [n] => f"single: {n}",
    [first, second] => f"pair: {first}, {second}",
    // !callout-center[/\.\.rest/] `Slice<int>`
    [first, ..rest] => f"first is {first}, {rest.length()} more",
  }
}
```

The rest pattern `..rest` captures the remaining elements. On a slice it gives a `Slice`, and on an array an `Array`. The rest pattern must come last:

```lisette
fn last(items: Slice<int>) -> int {
  match items {
    // !callout-error[/last/] error: elements after rest pattern are not supported
    [..rest, last] => last,
    _ => 0,
  }
}
```

Use `..` without an identifier to ignore the rest:

```lisette
fn first_or_zero(items: Slice<int>) -> int {
  match items {
    // !callout[/\.\./] ignores all remaining elements
    [first, ..] => first,
    [] => 0,
  }
}
```

## Or-patterns

An or-pattern uses `|` to join several alternatives in one arm:

```lisette
fn axis(direction: Direction) -> string {
  match direction {
    North | South => "north-south",
    East | West => "east-west",
  }
}
```

Alternatives can bind variables if all of them bind the same names:

```lisette
enum Event {
  KeyDown(rune),
  KeyUp(rune),
}

fn describe(event: Event) -> string {
  match event {
    // !callout-center[/(?<=\{)key/] `rune`
    KeyDown(key) | KeyUp(key) => f"key: {key}",
  }
}
```

## `as` for value capture

Use `as` to capture the entire matched value:

```lisette
fn track(history: Slice<Message>, msg: Message) -> Slice<Message> {
  match msg {
    // !callout-center[/(?<=as )moved/] `Message`
    Move { .. } as moved => history.append(moved),
    _ => history,
  }
}
```

For an arm with alternatives, place `as` on each one:

```lisette
fn tag(event: Event) -> (rune, Event) {
  match event {
    // !callout-center[/(?<=KeyDown\(key\) as )pressed/] `Event`
    // !callout-center[/(?<=KeyUp\(key\) as )pressed/] `Event`
    KeyDown(key) as pressed | KeyUp(key) as pressed => (key, pressed),
  }
}
```

## Pattern guards

Add `if` after a pattern to require an additional condition:

```lisette
fn sign(opt: Option<int>) -> string {
  match opt {
    Some(n) if n > 0 => "positive",
    Some(_) => "non-positive",
    None => "empty",
  }
}
```

A guard and `as` can appear in the same arm:

```lisette
fn positive(opt: Option<int>) -> Option<int> {
  match opt {
    // !callout-center[/found/] `Option<int>`
    Some(n) as found if n > 0 => found,
    _ => None,
  }
}
```

Guards do not count toward exhaustiveness. If all arms have guards, a wildcard or catch-all arm is still required.

## `let else`

`let else` binds a pattern that can fail to match. When the pattern does not match, the `else` branch runs. The `else` branch must `return`, `break`, `continue`, or `panic`.

```lisette
fn greet(names: Map<int, string>, id: int) -> string {
  let Some(name) = names.get(id) else { return "unknown user" }
  f"hello, {name}"
}
```

A slice pattern that constrains the length can fail, because a slice has no fixed length:

```lisette
fn run(args: Slice<string>) {
  let [_, command, ..] = args else {
    fmt.Println("usage: tool <command>")
    return
  }
  fmt.Println(f"running {command}")
}
```

