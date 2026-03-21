# Coming from Rust

Quick reference for Rust developers. For full details, see [`reference.md`](../reference/README.md)

Lisette has no ownership, borrowing, or lifetimes, as memory is garbage-collected.

Much of Lisette's syntax should feel familiar; the sections below cover where things diverge.

## Variables

```rust
let s1 = String::from("hello");
let s2 = s1;
println!("{}", s1); // error: value moved
```

```rust
let s1 = "hello"
let s2 = s1
fmt.Println(s1) // works
```

`let` and `let mut` work as expected. No moves, no `Copy` vs. `Clone` distinction. Semicolons are optional.

## References

```rust
let x = 42;
let r: &i32 = &x;
println!("{}", *r);

fn increment(r: &mut i32) {
    *r += 1;
}
```

```rust
let x = 42
let r: Ref<int> = &x
fmt.Println(r.*)

fn increment(r: Ref<int>) {
  r.* += 1
}
```

Lisette has a single reference type `Ref<T>` instead of Rust's `&T` and `&mut T`. Dereference with postfix `.*` as in Zig. Any `Ref<T>` can mutate the referenced value.

## Types

| Rust                     | Lisette                |
| ------------------------ | ---------------------- |
| `i32`, `i64`, etc.       | `int`, `int64`, etc.   |
| `u32`, `u64`, etc.       | `uint`, `uint64`, etc. |
| `f64`                    | `float64`              |
| `bool`                   | `bool`                 |
| `char`                   | `rune`                 |
| `String`, `&str`         | `string`               |
| `Vec<T>`                 | `Slice<T>`             |
| `HashMap<K, V>`          | `Map<K, V>`            |
| `Box<T>`, `&T`, `&mut T` | `Ref<T>`               |
| `Option<T>`              | `Option<T>`            |
| `Result<T, E>`           | `Result<T, E>`         |

## Error handling

`?`, `match`, `let else`, and `unwrap_or` work the same. Lisette has no `unwrap()`.

## Pattern matching

```rust
match color {
    Color::Red => println!("red"),
    Color::Blue => println!("blue"),
}
```

```rust
match color {
  Color.Red => fmt.Println("red"),
  Color.Blue => fmt.Println("blue"),
}
```

`.` instead of `::` for variant access. Lisette's prelude variants (`Some`, `None`, `Ok`, `Err`) need no prefix.

## Traits and interfaces

```rust
trait Display {
    fn to_string(&self) -> String;
}

impl Display for Point {
    fn to_string(&self) -> String {
        format!("({}, {})", self.x, self.y)
    }
}
```

```rust
interface Display {
  fn to_string(self) -> string
}

impl Point {
  fn to_string(self) -> string {
    f"({self.x}, {self.y})"
  }
}
```

Interfaces are implicitly satisfied, i.e. a type satisfies an interface simply by having matching methods. No explicit `impl Trait for Type` syntax.

## String formatting

```rust
let msg = format!("Hello, {}!", name);
```

```rust
let msg = f"Hello, {name}!"
```

## Pipeline operator

```rust
let result = "  Hello World  ".trim().to_lowercase().replace(" ", "-");
```

```rust
let result = "  Hello World  "
  |> strings.TrimSpace
  |> strings.ToLower
  |> strings.ReplaceAll(" ", "-")
```

No equivalent in Rust. `|>` passes the left side as the first argument to the right side.

## Modules

```rust
use crate::models;
let u = models::User { name: String::from("Alice") };
```

```rust
import "models"
let u = models.User { name: "Alice" }
```

Lisette modules are directories that you `import` by path. Imported items are namespaced.

## Concurrency

```rust
tokio::spawn(async {
    do_work().await;
});
let (tx, rx) = mpsc::channel();
```

```rust
task do_work()
let ch = Channel.new<int>()
let (tx, rx) = ch.split()
```

Go-style concurrency: `task` spawns a goroutine, and channels pass values between them.

## No derives

```rust
#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    age: i32,
}
```

```rust
#[json]
struct User {
  name: string,
  age: int,
}
```

Serialization uses built-in attributes: `#[json]`, `#[yaml]`, etc.
