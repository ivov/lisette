# Pattern Matching

Patterns destructure values and bind variables in a single step.

## Match expressions

`match` tests a value against patterns and runs the first matching arm:

```rust
fn describe(n: int) -> string {
  match n {
    0 => "zero",
    1 => "one",
    _ => "many",
  }
}
```

Each arm has a pattern, `=>`, and an expression. The entire `match` is an expression that produces the value of the matched arm.

## Pattern types

### Literals

Integer, boolean, string, and character literals match exact values:

```rust
match c {
  'a' => "letter a",
  'b' => "letter b",
  _ => "other",
}
```

### Bindings

An identifier binds the matched value to a name:

```rust
match opt {
  Some(x) => x * 2,
  None => 0,
}
```

### Wildcard

`_` matches any value without binding:

```rust
match opt {
  Some(_) => "has value",
  None => "empty",
}
```

### Tuples

Tuple patterns destructure by position:

```rust
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

```rust
struct Point { x: int, y: int }

match p {
  Point { x: 0, y: 0 } => "origin",
  Point { x, y: 0 } => f"on x-axis at {x}",
  Point { x, y } => f"at ({x}, {y})",
}
```

Use `..` to ignore remaining fields:

```rust
struct User { name: string, email: string, age: int }

match user {
  User { name, .. } => f"hello, {name}",
}
```

### Enum variants

Enum patterns match variants and destructure their payloads:

```rust
enum Message {
  Ready,
  Write(string),
  Move { x: int, y: int },
}

match msg {
  Message.Ready => "ready",
  Message.Write(text) => f"writing: {text}",
  Message.Move { x, y } => f"moving to ({x}, {y})",
}
```

### Slices

Slice patterns match elements:

```rust
match items {
  [] => "empty",
  [x] => f"single: {x}",
  [first, second] => f"pair: {first}, {second}",
  [first, ..rest] => f"first is {first}, {rest.length()} more",
}
```

The rest pattern `..rest` captures remaining elements as a slice. It must appear last; elements after `..` are not allowed.

Use `..` without an identifier to ignore the rest:

```rust
match items {
  [first, ..] => first,
  [] => 0,
}
```

## Or-patterns

Use `|` to match multiple patterns in one arm:

```rust
match dir {
  Direction.North | Direction.South => "vertical",
  Direction.East | Direction.West => "horizontal",
}
```

Or-patterns can bind variables if all alternatives bind the same names:

```rust
enum Event {
  KeyDown(rune),
  KeyUp(rune),
}

match event {
  Event.KeyDown(c) | Event.KeyUp(c) => f"key: {c}",
}
```

Or-patterns can only appear at the top level of a match arm. They cannot be nested inside other patterns.

## `as` bindings

Use `as` to capture the matched value:

```rust
match msg {
  Message.Move { x, .. } as m => log.insert(x, m),
  _ => {},
}
```

Captured values are available in guards:

```rust
match opt {
  Some(Point { x, .. }) as p if x > 0 => transform(p),
  _ => default,
}
```

For or-patterns, place `as` on each alternative:

```rust
match event {
  Event.KeyDown(c) as e | Event.KeyUp(c) as e => record(e, c),
}
```

`as` is only allowed in `match`, `if let`, and `while let`.

## Guards

Add `if` after a pattern to require an additional condition:

```rust
match opt {
  Some(x) if x > 0 => "positive",
  Some(_) => "non-positive",
  None => "empty",
}
```

Guards do not count toward exhaustiveness. If all arms have guards, a wildcard or catch-all arm is still required.

## Exhaustiveness

Patterns must cover all possible values:

```rust
enum Color { Red, Green, Blue }

match color {
  Color.Red => "red",
  Color.Green => "green",
}
```

The compiler enforces exhaustiveness:

```
  [error] Non-exhaustive match
   ╭─[example.lis:3:1]
 3 │ match color {
   · ───────┬───
   ·        ╰── not all patterns covered
 4 │   Color.Red => "red",
 5 │   Color.Green => "green",
 6 │ }
   ╰────
  help: Handle the missing case `Color.Blue`, e.g. `Color.Blue => { ... }`
```

## Destructuring in `let`

Patterns can destructure values in let bindings:

```rust
let (x, y) = get_point()
let Point { x, y } = p
let [first, ..rest] = items else { return }
```

Slice patterns are refutable because `Slice<T>` has unknown length — use `let else` to handle the non-matching case.

## `let else`

`let else` either matches or leads to an early exit:

```rust
fn process(opt: Option<int>) -> int {
  let Some(x) = opt else { return 0; };
  x * 2
}
```

The `else` block must `return`, `break`, or `continue`.

<br>

<table><tr>
<td>← <a href="07-pointers.md"><code>07-pointers.md</code></a></td>
<td align="right"><a href="09-error-handling.md"><code>09-error-handling.md</code></a> →</td>
</tr></table>
