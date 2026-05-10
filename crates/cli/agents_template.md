# Lisette

Lisette compiles to Go. Rust-like syntax, Go runtime. No ownership, no borrowing, no lifetimes.

## Lisette is NOT Rust

| Rust                      | Lisette                                                          |
| ------------------------- | ---------------------------------------------------------------- |
| `&T`, `&mut T`            | `Ref<T>`                                                         |
| `*ptr`                    | `ptr.*`                                                          |
| `Vec<T>`                  | `Slice<T>`                                                       |
| `HashMap<K, V>`           | `Map<K, V>`                                                      |
| `String`, `&str`          | `string`                                                         |
| `Color::Red`              | `Color.Red`                                                      |
| `trait Foo`               | `interface Foo`                                                  |
| `impl Foo for Bar`        | just implement the methods (structural)                          |
| `.unwrap()`               | DOES NOT EXIST — use `match`, `let else`, `?`, or `.unwrap_or()` |
| `println!("{}", x)`       | `fmt.Println(f"{x}")`                                            |
| `format!("...")`          | `f"..."`                                                         |
| `vec![1, 2, 3]`           | `[1, 2, 3]`                                                      |
| `v.push(x)`               | `v = v.append(x)` (returns new slice)                            |
| `v.len()`                 | `v.length()`                                                     |
| `.iter().map().collect()` | `.map()` directly on `Slice<T>`                                  |
| `Box<dyn Error>`          | `error` (lowercase, it's an interface)                           |

Prelude variants need no prefix: `Some(x)`, `None`, `Ok(x)`, `Err(e)`.

## Lisette is NOT Go

| Go                             | Lisette                                       |
| ------------------------------ | --------------------------------------------- |
| `func`                         | `fn`                                          |
| `x := 5`                       | `let x = 5`                                   |
| `var x int`                    | `let x: int = ...` (must initialize)          |
| `nil`                          | DOES NOT EXIST — use `None` for absent values |
| `switch`                       | `match` (exhaustive)                          |
| `go func() { ... }()`          | `task { ... }`                                |
| `make(chan int)`               | `Channel.new<int>()`                          |
| `ch <- v`                      | `ch.send(v)`                                  |
| `v := <-ch`                    | `ch.receive()` returns `Option<T>`            |
| `if err != nil { return err }` | `let x = fallible()?`                         |
| `(T, error)` return            | `Result<T, error>`                            |
| `(T, error)` non-exclusive     | `Partial<T, error>`                           |
| `(T, bool)` return             | `Option<T>`                                   |
| `for i := 0; i < n; i++`       | `for i in 0..n`                               |
| `for {}`                       | `loop {}`                                     |
| `f(args...)` (variadic spread) | `f(args...)`                                  |

Variables are immutable by default. Use `let mut` for mutable bindings.

## String literals

`"..."` strings can span multiple lines — newlines and indentation are preserved verbatim. Use `r"..."` for raw strings (no escape processing), useful for regex, Windows paths, or any text with backslashes.

## Zero-filling struct literals

Lisette requires all struct fields to be initialized. Use `..` to fill any unspecified fields with their zero value: `Point { x: 10, .. }` leaves `y` at its zero value, `Point { .. }` zero-fills everything. Useful for Go structs with many config fields.

## Partial results

A few Go functions return `(T, error)` where both can be meaningful at once, e.g. `io.Reader.Read`. These bind as `Partial<T, E>` with three variants: `Partial.Ok(T)`, `Partial.Err(E)`, `Partial.Both(T, E)`. Match all three; `?` does not work on `Partial`.

## Project structure

```
my_project/
├── lisette.toml         // project manifest
├── AGENTS.md
└── src/
    ├── main.lis         // entry point
    └── models/          // module named "models"
        └── user.lis
```

All `.lis` files go in `src/`. Subdirectories are modules, imported by path: `import "models"`.

## Go interop

```
import "go:fmt"          // Go stdlib — always prefix with go:
import "go:net/http"     // nested packages too
import "models"          // Lisette module — no prefix
```

NEVER convert Go function names to snake_case. Keep PascalCase:

```
strings.ToUpper("hello")     // correct
strings.to_upper("hello")    // WRONG
```

Go type mappings:

| Go                   | Lisette                                            |
| -------------------- | -------------------------------------------------- |
| `*T`                 | `Ref<T>` (non-null) or `Option<Ref<T>>` (nullable) |
| `[]T`                | `Slice<T>`                                         |
| `map[K]V`            | `Map<K, V>`                                        |
| `chan T`             | `Channel<T>`                                       |
| `any`, `interface{}` | `Unknown` (narrow with `assert_type<T>()`)         |
| `...T`               | `VarArgs<T>`                                       |

## Complete example

```
import "go:fmt"
import "go:net/http"
import "go:io"
import "go:strings"
import "go:errors"

#[json]
struct User {
  name: string,
  #[json(omitempty)]
  email: Option<string>,
}

enum Role {
  Admin,
  Member { team: string },
}

impl User {
  fn display(self) -> string {
    f"User({self.name})"
  }

  fn set_email(self: Ref<User>, email: string) {
    self.email = Some(email)
  }
}

fn fetch_body(url: string) -> Result<string, error> {
  let resp = http.Get(url)?
  defer resp.Body.Close()
  let body = io.ReadAll(resp.Body)?
  Ok(body as string)
}

fn find_admin(users: Slice<User>) -> Result<User, error> {
  let Some(user) = users.find(|u| u.name == "admin") else {
    return Err(errors.New("no admin found"))
  };
  Ok(user)
}

fn main() {
  let users = [
    User { name: "Alice", email: Some("alice@co.com") },
    User { name: "Bob", email: None },
  ]

  let names = users
    .map(|u| u.name)
    .filter(|n| n.length() > 3)

  for name in names {
    let slug = name |> strings.ToLower |> strings.TrimSpace
    fmt.Println(f"slug: {slug}")
  }

  match find_admin(users) {
    Ok(admin) => fmt.Println(admin.display()),
    Err(e) => fmt.Println(f"error: {e}"),
  }

  let ch = Channel.new<int>()
  task { ch.send(42) }
  match ch.receive() {
    Some(v) => fmt.Println(v),
    None => {},
  }

  let result = recover { fetch_body("http://example.com") }
  match result {
    Ok(body) => fmt.Println(body),
    Err(pv) => fmt.Println(pv.message()),
  }
}
```

The last expression in a block is the return value. Do NOT add a trailing semicolon.

## Documentation

Full reference: https://github.com/ivov/lisette/tree/main/docs/reference

## Commands

- `lis run` — compile and run
- `lis build` — compile to `target/`
- `lis check` — type check only
- `lis format` — format code
- `lis add google/uuid` - add a third-party Go dependency
- `lis sync` - reconcile `lisette.toml` with imports
- `lis doc Slice` — show a prelude type and its methods
- `lis doc Option.map` — show a single method
- `lis doc go:os` — browse a Go stdlib package
- `lis doc -s split` — search prelude and Go stdlib

## Boundaries

NEVER modify anything inside `target/` — all generated by the compiler.
