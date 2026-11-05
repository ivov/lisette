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
| `mut` | Marks a mutable binding or writable type |
| `const` | Defines a compile-time constant |
| `fn` | Defines a function or method |
| `struct` | Defines a type with fields |
| `enum` | Defines a type with variants |
| `interface` | Defines a set of methods a type can satisfy |
| `impl` | Defines methods on a type |
| `type` | Defines a type or type alias |
| `as` | Converts between types, or names a pattern |

### Control flow

| Keyword | Purpose |
| --- | --- |
| `if` | Takes a branch when a condition holds |
| `else` | Takes the remaining branch |
| `match` | Selects a branch by pattern matching |
| `for` | Iterates over values |
| `in` | Names what a `for` loop iterates |
| `while` | Repeats while a condition holds |
| `loop` | Repeats until a `break` |
| `break` | Exits the enclosing loop |
| `continue` | Skips to the next iteration |
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

## Identifiers

An identifier is a name given to a declaration. It starts with a letter or underscore, followed by any number of letters, digits, or underscores. Identifiers are case-sensitive.

```plaintext
// !callout-right variable, function, parameter
foo
// !callout-right type, type parameter
Point
// !callout-right constant
MAX_SIZE
// !callout-right discarded
_count
```

A lone `_` is not an identifier. It discards a value, and later code cannot read it.

## Literals

### Numerics

Integer literals are of type `int`.

```lisette
let count = 42
```

Underscore separators improve readability. They cannot be leading, trailing, or consecutive.

```lisette
// !callout-ok-right separator
1_000
// !callout-right identifier
_1000
// !callout-error-right error: trailing underscore
1000_
// !callout-error-right error: consecutive underscores
1__000
```

Hex, octal, and binary literals use prefixes `0x`, `0o`, and `0b`. Leading-zero octal is not allowed.

```lisette
let hex = 0xFF
let octal = 0o755
let binary = 0b1010_0001
// !callout-error-right error: leading zero
let octal = 0755
```

Float literals are of type `float64`. A decimal point requires digits on both sides.

```lisette
let pi = 3.14159
let half = 0.5
```

Exponent notation uses `e` with an optional sign.

```lisette
let small = 1.5e-3
let large = 6.02e23
```

An `i` suffix on a decimal literal creates an imaginary value, for use with `complex64` and `complex128`.

```lisette
let unit = 1i
let point = 3 + 4i
```

### Booleans

Boolean literals are of type `bool`.

```lisette
let yes = true
let no = false
```

### Strings

String literals are of type `string`. They are enclosed in double quotes and may be multiline.

```lisette
let greeting = "Hello, world!"
let escaped = "line one\nline two"
let quoted = "She said \"hi\""
let multiline = "This is
a very long
multiline string."
```

A newline between the opening and closing `"` is preserved in the value as a `\n` byte. Source-code indentation inside a multiline string is part of the value.

Escape sequences:

| Sequence     | Meaning                              |
| ------------ | ------------------------------------ |
| `\\`         | Backslash                            |
| `\"`         | Double quote                         |
| `\n`         | Newline                              |
| `\r`         | Carriage return                      |
| `\t`         | Tab                                  |
| `\0`         | Null                                 |
| `\a`         | Alert                                |
| `\b`         | Backspace                            |
| `\f`         | Form feed                            |
| `\v`         | Vertical tab                         |
| `\xHH`       | Hex value, two digits                |
| `\ooo`       | Octal value, three digits            |
| `\u{HEX}`    | Unicode code point, any hex digits   |
| `\UHHHHHHHH` | Unicode code point, eight hex digits |

#### Raw strings

Raw strings begin with `r"` and treat backslashes as plain characters.

```lisette
let pattern = r"([a-zA-Z])(\d)"
let path = r"C:\Users\me"
let block = r"line one
line two"
```

Raw strings cannot contain `"`. For text with quotes, use a regular string with `\"`.

#### Format strings

Format strings begin with `f"` and accept interpolations in `{}`.

```lisette
let greeting = f"Hello, {user.name}! You have {user.unread} new messages."
let total = f"Total: {price * quantity}"
```

Use `{{` and `}}` to escape braces.

```lisette
// !callout-right `{name} is Alice`
let text = f"{{name}} is {user.name}"
```

### Runes

Runes are enclosed in single quotes.

```lisette
let c = 'a'
let newline = '\n'
let null = '\0'
```

Runes use the same escape sequences, with `\'` in place of `\"`.

### Slices and arrays

Slice literals are of type `Slice`. An annotation or a parameter can declare them as `Array` instead. They are enclosed in square brackets, and all elements must have the same type.

```lisette
// !callout-right inferred as `Slice<int>`
let nums = [1, 2, 3]
// !callout-right annotated as `Array<int, 2>`
let pair: Array<int, 2> = [1, 2]
// !callout-right ambiguous, requires annotation
let empty: Slice<int> = []
```

### Tuples

Tuple literals are of type `(A, B, ...)`, with one type per element. They are enclosed in round brackets and hold two to five elements, accessed by position.

```lisette
let pair = (42, "hello")
// !callout-right `42`
let first = pair.0
// !callout-right `"hello"`
let second = pair.1

let triple = (1, true, "three")
// !callout-right `a` is `1`, `b` is `true`, `c` is `"three"`
let (a, b, c) = triple
```

For more than five elements, use a [struct](/docs/structs/) with named fields.

## Trivia

### Comments

Line comments start with `//` and extend to the end of the line.

```lisette
let timeout = 30 // seconds
```

Doc comments start with `///` and document the item that follows.

```lisette
/// Returns the sum of two integers.
fn add(a: int, b: int) -> int {
  a + b
}
```

File comments start with `//!` at the top of the file. They are preserved at the top of the generated Go file.

```lisette
//! Copyright 2026 Acme Corp.
//! SPDX-License-Identifier: Apache-2.0

import "strings"
```

### Shebang

A shebang starts with `#!` at the top of the file and makes the file directly runnable.

```lisette
#!/usr/bin/env -S lis run

import "go:fmt"
import "go:os"

fn main() {
  fmt.Println(f"Hello, {os.Args[1]}!")
}
```

```sh
chmod +x greet.lis && ./greet.lis alice
```

📚 See [Scripts](/docs/scripts/)

### Semicolons

Semicolons are never required. `lis format` removes any you write.

