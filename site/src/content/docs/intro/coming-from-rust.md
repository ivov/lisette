---
title: "Coming from Rust"
description: "Where Lisette reads like Rust and behaves differently"
---

Every section below is a place where Lisette reads like Rust and behaves differently.

## Bindings
```rust file="bindings.rs"
let s1 = String::from("hello");
let s2 = s1;
// !callout-error-right error: value moved
println!("{}", s1);
```

```lisette file="bindings.lis"
let s1 = "hello"
let s2 = s1
// !callout-ok-right
fmt.Println(s1)
```

Rust moves `s1` into `s2`. Lisette has no moves, so `s1` stays usable.

📚 See [Bindings](/docs/bindings/)


## Strings
```rust file="greet.rs"
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
```

```lisette file="greet.lis"
fn greet(name: string) -> string {
  f"Hello, {name}!"
}
```

`String` and `&str` collapse into one `string`. No owned versus borrowed distinction.

📚 See [String](/docs/prelude/strings/)


## References
```rust file="increment.rs"
let x = 42;
let r: &i32 = &x;
println!("{}", *r);

fn increment(r: &mut i32) {
    *r += 1;
}
```

```lisette file="increment.lis"
let x = 42
let r: Ref<int> = &x
fmt.Println(r.*)

fn increment(r: mut Ref<int>) {
  r.* += 1
}
```

`Ref<T>` and `mut Ref<T>` line up with Rust's `&T` and `&mut T`, but `mut Ref<T>` is not exclusive. `&x` yields whatever `x` allows, so a `let mut` binding gives a `mut Ref<T>` and a plain `let` gives a read-only one.

📚 See [References](/docs/references/)


## Write permission
```rust file="alias.rs"
let a = vec![1, 2, 3];
let mut b = a;
b[0] = 99;
// !callout-error-right error: borrow of moved value
println!("{:?}", a);
```

```lisette file="alias.lis"
let a = [1, 2, 3]
// !callout-ok-right allowed, but `b` inherits `a`'s permission
let mut b = a
// !callout-error-right error: `b` is read-only
b[0] = 99

// !callout-right independent storage
let mut c = a.clone()
// !callout-ok-right write permitted, `a` untouched
c[0] = 99
```

A slice is a handle. Both names reach the same storage rather than a copy of it, and `b` inherits the read-only permission of `a`. `.clone()` severs the alias and hands back writable storage.

```rust file="index.rs"
struct Index {
    counts: HashMap<String, i32>,
    tags: Vec<String>,
}
```

```lisette file="index.lis"
struct Index {
  // !callout-right writable, through a writable `Index`
  counts: mut Map<string, int>,
  // !callout-right read-only
  tags: Slice<string>,
}
```

Rust makes a whole value mutable at once, through `&mut self` or a `mut` binding. In Lisette, each field declares its own permission.

📚 See [Bindings](/docs/bindings/#write-permission-in-bindings), [Structs](/docs/structs/#write-permission-in-fields) and [Safety](/docs/intro/safety/#aliased-collections)


## Closures
```rust file="closure.rs"
let mut total = 0;
// !callout-error-right error: cannot borrow `add` as mutable
let add = |n: i32| { total += n; };
add(5);
```

```lisette file="closure.lis"
let mut total = 0
let add = |n: int| { total += n }
add(5)
```

A closure captures by reference, and the garbage collector keeps what it captured alive. No borrow rules, and no `move` keyword.

📚 See [Lambdas](/docs/functions/#lambdas)


## Traits and interfaces
```rust file="display.rs"
trait Display {
    fn to_string(&self) -> String;
}

impl Display for Point {
    fn to_string(&self) -> String {
        format!("({}, {})", self.x, self.y)
    }
}
```

```lisette file="display.lis"
interface Display {
  fn to_string() -> string
}

// !callout-right `Point` satisfies `Display`
impl Point {
  fn to_string(self) -> string {
    f"({self.x}, {self.y})"
  }
}
```

In Lisette, a type satisfies an interface simply by having matching methods.

📚 See [Interfaces](/docs/interfaces/)


## Equality
```rust file="equality.rs"
#[derive(PartialEq)]
struct Order {
    id: i32,
    tags: Vec<String>,
}
```

```lisette file="equality.lis"
#[equality]
struct Order {
  pub id: int,
  pub tags: Slice<string>,
}
```

Rust needs the derive before `==` works. Lisette accepts `==` without it, but only if every field is comparable. A slice, map or function field removes `==`, and `#[equality]` supplies an `equals()` method in its place.

📚 See [Equality](/docs/attributes/#equality)


## Error handling
```rust file="read.rs"
let bytes = std::fs::read(path).unwrap();
```

```lisette file="read.lis"
let Ok(bytes) = os.ReadFile(path) else {
  fmt.Println("could not read")
  return
}
```

`?`, `match`, `let else`, and `map_or` work the same. Lisette has no `unwrap()`.

📚 See [Failures](/docs/failures/)
