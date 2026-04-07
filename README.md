# Lisette

[![crates.io](https://img.shields.io/crates/v/lisette.svg?logo=rust)](https://crates.io/crates/lisette)
[![Go](https://img.shields.io/badge/Go-1.25+-00ADD8?logo=go)](https://go.dev)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Little language inspired by Rust that compiles to Go.

Safe and expressive:

- Hindley-Milner type system
- Algebraic data types, pattern matching
- Expression-oriented, immutable by default
- Rust-like syntax plus `|>` operator and `try` blocks
- Go-style interfaces, channels, goroutines

Quietly practical:

- Interop with Go standard library
- Linter, formatter, 250+ diagnostics
- Fast incremental compiler, readable Go
- LSP support for VSCode, Neovim, Zed, Helix

## Quick tour

Enums and pattern matching:

```rust
enum Shape {
  Circle(float64),
  Rectangle { width: float64, height: float64 },
}

fn area(shape: Shape) -> float64 {
  match shape {
    Shape.Circle(r) => 3.14 * r * r,
    Shape.Rectangle { width, height } => width * height,
  }
}
```

Go interop and `?` for error handling:

```rust
import "go:os"
import "go:io"
import "go:fmt"

fn load_config(path: string) -> Result<Cfg, error> {
  let file = os.Open(path)?
  defer file.Close()
  let data = io.ReadAll(file)?
  parse_yaml(data)
}

fn main() {
  match load_config("app.yaml") {
    Ok(config) => start(config),
    Err(e) => fmt.Println("error:", e),
  }
}
```

`Option` instead of `nil` and zero values:

```rust
match flag.Lookup("verbose") {
  Some(f) => fmt.Println(f.Value),
  None => fmt.Println("no such flag"),
}

let scores = Map.new<string, int>()
match scores.get("alice") {
  Some(score) => fmt.Println(score),
  None => fmt.Println("score not found"),
}
```

Pipeline operator:

```rust
let slug = "  Hello World  "
  |> strings.TrimSpace
  |> strings.ToLower
  |> strings.ReplaceAll(" ", "-")  // "hello-world"
```

Typed channels and concurrent tasks:

```rust
let (tx, rx) = Channel.new<string>().split()

task {
  tx.send("hello")
  tx.close()
}

match rx.receive() {
  Some(msg) => fmt.Println(msg),
  None => fmt.Println("closed"),
}
```

## Learn more

- 💡 [`quickstart.md`](docs/intro/quickstart.md) — Set up a Lisette project
- 🧿 [`safety.md`](docs/intro/safety.md) — Go issues Lisette prevents
- 📚 [`reference.md`](docs/reference/README.md) — Full language reference
- 🌎 [`roadmap.md`](docs/intro/roadmap.md) — Status and planned work

## Author

© 2026 Iván Ovejero
