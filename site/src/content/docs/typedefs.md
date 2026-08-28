---
title: "Typedefs"
description: "How Lisette reads Go packages, and where its signatures differ from Go's"
---

A type definition is a `.d.lis` file that describes a Go package in Lisette syntax.

```go file="strconv.go"
// !callout[/Atoi/] Go function signature
func Atoi(s string) (int, error)
```

```lisette file="strconv.d.lis"
// !callout[/Atoi/] Lisette equivalent
pub fn Atoi(s: string) -> Result<int, error>
```

Typedefs are written by the [compiler](https://github.com/ivov/lisette/tree/main/bindgen#bindgen), never by hand.

- Go stdlib typedefs are built into the compiler.
- For third-party dependencies, `lis add` writes typedefs to `target/.lisette/typedefs` in your project.

To inspect a typedef in your IDE, use Go to Definition on any imported Go symbol.

## Type equivalents

Typedefs describe many Go types identically in Lisette:

| Go                             | Lisette                        |
| ------------------------------ | ------------------------------ |
| `string`                       | `string`                       |
| `bool`                         | `bool`                         |
| `error` interface              | `error` interface              |
| `int`, `int8`, `int16`, etc.   | `int`, `int8`, `int16`, etc.   |
| `uint`, `uint8`, `uint16`, etc. | `uint`, `uint8`, `uint16`, etc. |
| `float32`, `float64`           | `float32`, `float64`           |
| `complex64`, `complex128`      | `complex64`, `complex128`      |

Other Go types are described differently:

| Go                         | Lisette                      |
| -------------------------- | ---------------------------- |
| `(T, error)`               | `Result<T, error>`           |
| `error` as the only return | `Result<(), error>`          |
| `(T, bool)`                | `Option<T>` or `(T, bool)`   |
| `*T`                       | `Ref<T>` or `Option<Ref<T>>` |
| `any`, `interface{}`       | `Unknown`                    |
| `[]T`                      | `Slice<T>`                   |
| `[N]T`                     | `Array<T, N>`                |
| `map[K]V`                  | `Map<K, V>`                  |
| `...T`                     | `VarArgs<T>`                 |
| `chan T`                   | `Channel<T>`                 |
| `type X int64`             | `pub struct X(int64)`        |

🐙 See the [bindings generator](https://github.com/ivov/lisette/tree/main/bindgen#bindgen) for the complete list.
