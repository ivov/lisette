# Lexical Structure

## Keywords

These 28 words are reserved and cannot be used as identifiers:

```
as        break     const     continue    defer
else      enum      fn        for         if
impl      import    in        interface   let
loop      match     mut       pub         recover
return    select    struct    task        try
type      while
```

`true` and `false` are boolean literals, not keywords.

## Identifiers

An identifier starts with a letter or underscore, followed by any number of letters, digits, or underscores. Identifiers are case-sensitive.

```
foo
_count
Point
MAX_SIZE
```

The bare underscore `_` is a wildcard pattern, not a usable identifier. It can appear in patterns but cannot be bound or referenced.

## Literals

### Integer literals

Integer literals have type `int`.

```rust
let decimal = 42
let with_separators = 1_000_000
let hex = 0xFF
let octal = 0o755
let binary = 0b1010_0001
```

Digit separators (`_`) improve readability. They cannot be leading, trailing, or consecutive: `1_000` is valid; `1__000`, `1000_`, and `_1000` are not.

Hex, octal, and binary literals use prefixes `0x`, `0o`, and `0b` (case-insensitive). Legacy octal syntax with a leading zero (`0755`) is also accepted.

### Float literals

Float literals have type `float64`. A decimal point requires digits on both sides.

```rust
let pi = 3.14159
let half = 0.5
let sci = 1.5e-3
```

Exponent notation uses `e` with an optional sign.

### Imaginary literals

An `i` suffix on a decimal numeric literal creates an imaginary value, for use with `complex64` and `complex128`.

```rust
let im = 4i
let im_float = 3.14i
```

Only decimal literals support the `i` suffix.

### Boolean literals

```rust
let yes = true
let no = false
```

### String literals

String literals are enclosed in double quotes and must be on a single line. Type: `string`.

```rust
let greeting = "Hello, world!"
let escaped = "line one\nline two"
let quoted = "She said \"hi\""
```

Escape sequences:

| Sequence | Meaning         |
| -------- | --------------- |
| `\\`     | Backslash       |
| `\"`     | Double quote    |
| `\n`     | Newline         |
| `\r`     | Carriage return |
| `\t`     | Tab             |

### Format strings

A format string begins with `f"` and can contain interpolated expressions in `{}`.

```rust
let name = "Alice"
let age = 30
let msg = f"Hello, {name}! You are {age} years old."
```

Use `{{` and `}}` to escape braces.

### Character literals

Character literals are enclosed in single quotes.

```rust
let c = 'a'
let newline = '\n'
let null = '\0'
```

Escape sequences: `\\`, `\'`, `\n`, `\r`, `\t`, `\0`.

### Slice literals

A slice literal is a comma-separated list of values in square brackets. All elements must have the same type.

```rust
let nums = [1, 2, 3]
let empty: Slice<int> = []
let nested = [[1, 2], [3, 4]]
```

## Comments

Line comments start with `//` and extend to the end of the line.

```rust
let x = 42 // a comment
```

Doc comments start with `///` and document the item that follows.

```rust
/// Returns the sum of two integers.
fn add(a: int, b: int) -> int {
  a + b
}
```

## Semicolons

Semicolons separate statements but are almost always optional. The compiler inserts them automatically at line boundaries.

```rust
// Semicolons inserted automatically
let x = 1
let y = 2

// Explicit semicolons for multiple statements on one line
let a = 1; let b = 2

// Expressions can span lines before a continuing operator
let result = items
  |> filter(is_valid)
  |> map(transform)

let total = first_value
  + second_value
  + third_value

let chained = something
  .method_one()
  .method_two()
```

<br>

<table><tr>
<td align="right"><a href="02-types.md"><code>02-types.md</code></a> →</td>
</tr></table>
