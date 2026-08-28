---
title: "Lexemes"
description: "Keywords, identifiers, literals, trivia"
---

## Keywords

A keyword is a word reserved by the language for its own syntax.

### Declarations

| Keyword | Purpose |
| --- | --- |
| `let` | Binds a value to a name inside a function |
| `mut` | Marks a binding or a parameter as mutable |
| `const` | Defines a compile-time constant of a primitive type |
| `fn` | Defines a function or a method |
| `struct` | Defines a record type with fields |
| `enum` | Defines a type with variants |
| `interface` | Defines a set of methods a type can satisfy |
| `impl` | Contains methods implemented by a type |
| `type` | Defines a named type or a type alias |

### Control flow

| Keyword | Purpose |
| --- | --- |
| `if` | Takes a branch when a condition holds |
| `else` | Takes the remaining branch |
| `match` | Selects a branch by pattern |
| `for` | Iterates over values |
| `in` | Names what a `for` loop iterates |
| `while` | Repeats while a condition holds |
| `loop` | Repeats until a `break` |
| `break` | Exits the enclosing loop |
| `continue` | Starts the next iteration of the loop |
| `return` | Exits the enclosing function |

### Failure handling

| Keyword | Purpose |
| --- | --- |
| `try` | Scopes `?` to a block |
| `defer` | Runs an expression on function exit |
| `recover` | Catches a panic |
| `assert` | States an expected result in a test |

### Concurrency

| Keyword | Purpose |
| --- | --- |
| `task` | Starts a concurrent task (goroutine) |
| `select` | Waits on several channel operations |

### Packages

| Keyword | Purpose |
| --- | --- |
| `import` | Brings a package into scope |
| `pub` | Makes an item visible outside its package |
| `as` | Converts between types, or names a pattern |

## Identifiers

An identifier is a name given to a declaration, such as a variable, a function, or a type. It starts with a letter or underscore, followed by any number of letters, digits, or underscores. Identifiers are case-sensitive.

```plaintext
// !callout-right variables, functions, parameters
foo
// !callout-right types, type parameters
Point
// !callout-right constants
MAX_SIZE
// !callout-right discarded
_count
```

The bare underscore `_` is a wildcard pattern, not a usable identifier.

## Literals

### Numerics

Integer literals have type `int`.

```lisette
let decimal = 42
let with_separators = 1_000_000
let hex = 0xFF
let octal = 0o755
let binary = 0b1010_0001
```

Underscore separators improve readability. They cannot be leading, trailing, or consecutive.

```lisette
// !callout-right leading, parses as identifier
_1000
// !callout-error-right error: trailing
1000_
// !callout-error-right error: consecutive
1__000
// !callout-ok-right
1_000
```

Hex, octal, and binary literals use prefixes `0x`, `0o`, and `0b` (case-insensitive). Legacy leading-zero octal (`0755`) is not allowed. Use the `0o` prefix (`0o755`) instead.

Float literals have type `float64`. A decimal point requires digits on both sides. Exponent notation uses `e` with an optional sign.

```lisette
let pi = 3.14159
let half = 0.5
let sci = 1.5e-3
```

An `i` suffix on a decimal numeric literal creates an imaginary value, for use with `complex64` and `complex128`. Only decimal literals support the `i` suffix.

```lisette
let im = 4i
let im_float = 3.14i
```

### Booleans

```lisette
let yes = true
let no = false
```

### Strings

String literals are enclosed in double quotes and may span multiple lines. Type: `string`. 

```lisette
let greeting = "Hello, world!"
let escaped = "line one\nline two"
let quoted = "She said \"hi\""
let multiline = "This is
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

#### Raw strings

A raw string literal begins with `r"` and ends with `"`. Inside, every character is literal, i.e. backslashes are not escapes. A raw string may span multiple lines.

```lisette
let pattern = r"([a-zA-Z])(\d)"
let path = r"C:\Users\me"
let block = r"line one
line two"
```

Raw strings cannot contain a double quote. Escape it as `\"` in a regular string instead.

#### Format strings

A format string begins with `f"` and can contain interpolated expressions in `{}`. The text portions follow the same multi-line rules as regular strings. Interpolation expressions inside `{}` must remain on a single line.

```lisette
let name = "Alice"
let age = 30
let msg = f"Hello, {name}! You are {age} years old."
let multiline = f"name: {name}
age: {age}"
```

Use `{{` and `}}` to escape braces.

### Runes

Rune literals are enclosed in single quotes.

```lisette
let c = 'a'
let newline = '\n'
let null = '\0'
```

Escape sequences: `\\`, `\'`, `\n`, `\r`, `\t`, `\0`.

### Slices and arrays

In a bracketed sequence of values, all elements must have the same type. This sequence is a `Slice` by default, or an `Array` when an array type is expected.

```lisette
// !callout-right inferred as `Slice<int>`
let nums = [1, 2, 3]
// !callout-right inferred type overridden
let address: Array<byte, 4> = [127, 0, 0, 1]
// !callout-right empty requires annotation
let empty: Slice<int> = []
```

## Trivia

### Comments

Line comments start with `//` and extend to the end of the line.

```lisette
let x = 42 // a comment
```

Doc comments start with `///` and document the item that follows.

```lisette
/// Returns the sum of two integers.
fn add(a: int, b: int) -> int {
  a + b
}
```

File comments start with `//!` and document the file itself. They form one contiguous block at the top of the file. Nothing but a shebang may come before it, and its content is emitted at the top of the generated Go file.

```lisette
//! Copyright 2026 Acme Corp.
//! SPDX-License-Identifier: Apache-2.0

import "strings"
```

### Shebang

At the very start of a file, a `#!` plus an interpreter is a shebang line, which makes a [script](/docs/scripts/) directly executable on Unix.

```lisette
#!/usr/bin/env -S lis run

import "go:fmt"

fn main() {
  fmt.Println("hi")
}
```

```sh
chmod +x greet.lis && ./greet.lis
```

### Semicolons

Semicolons are never required. `lis format` removes any you write.

