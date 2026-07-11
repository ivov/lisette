# Lexical Structure

## Keywords

These 29 words are reserved and cannot be used as identifiers:

```
as        assert    break     const       continue
defer     else      enum      fn          for
if        impl      import    in          interface
let       loop      match     mut         pub
recover   return    select    struct      task
try       type      var       while
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

Hex, octal, and binary literals use prefixes `0x`, `0o`, and `0b` (case-insensitive). Legacy leading-zero octal (`0755`) is not allowed; use the `0o` prefix (`0o755`) instead.

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

String literals are enclosed in double quotes and may span multiple lines. Type: `string`. 

```rust
let greeting = "Hello, world!"
let escaped = "line one\nline two"
let quoted = "She said \"hi\""
let multi = "This is
a very long
multiline string."
```

A newline between the opening and closing `"` is preserved in the value as a `\n` byte. Source-code indentation inside a multi-line string is part of the value.

Escape sequences:

| Sequence | Meaning         |
| -------- | --------------- |
| `\\`     | Backslash       |
| `\"`     | Double quote    |
| `\n`     | Newline         |
| `\r`     | Carriage return |
| `\t`     | Tab             |

### Raw string literals

A raw string literal begins with `r"` and ends with `"`. Inside, every character is literal, i.e. backslashes are not escapes. A raw string may span multiple lines.

```rust
let pattern = r"([a-zA-Z])(\d)"
let path    = r"C:\Users\me"
let block   = r"line one
line two"
```

Raw strings cannot contain a double quote; escape it as `\"` in a regular string instead.

### Format strings

A format string begins with `f"` and can contain interpolated expressions in `{}`. The text portions follow the same multi-line rules as regular strings. Interpolation expressions inside `{}` must remain on a single line.

```rust
let name = "Alice"
let age = 30
let msg = f"Hello, {name}! You are {age} years old."
let block = f"name: {name}
age: {age}"
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

### Slices and array literals

In a bracketed sequence of values, all elements must have the same type. This sequence is a `Slice` by default, or an `Array` when an array type is expected.

```rust
let nums = [1, 2, 3] // Slice<int>
let address: Array<byte, 4> = [127, 0, 0, 1]
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
