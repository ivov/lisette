# Coming from Go

Quick reference for Go developers. For full details, see [`reference.md`](../reference/README.md)

Every section below is a Go pattern with a Lisette equivalent.

## Variables

```go
x := 5          // mutable
x = 6           // mutated
var y int       // mutable, zero-initialized
```

```rust
let x = 5       // immutable
x = 6           // error: mutation disallowed

let mut y = 5   // mutable
y = 6           // mutated

let z: int      // error: must initialize
```

Variables are immutable by default. Use `let mut` for mutable bindings.

## Functions

```go
func add(a int, b int) int {
    return a + b
}
```

```rust
fn add(a: int, b: int) -> int {
  a + b
}
```

The last expression is the return value. Use `return` for early exits.

## Lambdas

```go
double := func(x int) int {
    return x * 2
}
nums = filter(nums, func(x int) bool {
    return x > 0
})
```

```rust
let double = |x: int| x * 2

let nums = nums.filter(|x| x > 0)
```

Parameters go between `|` pipes. Types can be inferred.

## Enums

Go has no equivalent. Lisette enums can carry data:

```rust
enum Result<T, E> {
  Ok(T),
  Err(E),
}

match r {
  Ok(value) => process(value),
  Err(e) => handle(e),
}
```

This is how `Option` and `Result` work. Lisette has no `nil` type.

## Pattern matching

```go
switch dir {
case "north", "south":
    fmt.Println("vertical")
case "east", "west":
    fmt.Println("horizontal")
default:
    fmt.Println("unknown")
}
```

```rust
match dir {
  "north" | "south" => fmt.Println("vertical"),
  "east" | "west" => fmt.Println("horizontal"),
  _ => fmt.Println("unknown"),
}
```

Lisette enforces exhaustiveness in `match` statements.

## Error handling

```go
result, err := doSomething()
if err != nil {
    return nil, err
}
process(result)
```

```rust
let result = do_something()?
process(result)
```

The `?` operator unwraps `Ok` or returns early with `Err`. Functions returning `(T, error)` in Go become `Result<T, error>` in Lisette.

## `nil` safety

```go
var user *User // pointer can be `nil`
if user != nil {
    fmt.Println(user.Name)
}
```

```rust
let user = get_user(id) // `Option<Ref<User>>`
if let Some(u) = user {
  fmt.Println(u.name)
}
```

`Ref<T>` is guaranteed non-`nil`. Nilable pointers become `Option<Ref<T>>`.

## Pointers

```go
x := 42
p := &x
fmt.Println(*p)
```

```rust
let x = 42
let p = &x
fmt.Println(p.*)
```

Dereference with postfix `.*`

## Structs

```go
type User struct {
    Name  string
    email string // unexported
}

u := User{Name: "Alice", email: "a@b.com"}
```

```rust
struct User {
  pub name: string,
  email: string,  // private
}

let u = User { name: "Alice", email: "a@b.com" }
```

Fields are private by default. Use `pub` to export.

## Methods

```go
func (r Rectangle) Area() float64 {
    return r.Width * r.Height
}

func (r *Rectangle) Scale(factor float64) {
    r.Width *= factor
    r.Height *= factor
}
```

```rust
impl Rectangle {
  fn area(self) -> float64 {
    self.width * self.height
  }

  fn scale(self: Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

Methods live in `impl` blocks. Use `self` for value receiver, `self: Ref<T>` for pointer receiver.

## Interfaces

```go
type Reader interface {
    Read(p []byte) (n int, err error)
}

type ReadWriter interface {
    Reader
    Writer
}
```

```rust
interface Reader {
  fn Read(self, p: Slice<byte>) -> Result<int, error>
}

interface ReadWriter {
  impl Reader
  impl Writer
}
```

Lisette uses structural typing for interfaces, like Go.

## Collections

```go
nums := []int{1, 2, 3}
nums = append(nums, 4)

ages := make(map[string]int)
ages["Alice"] = 20
age, ok := ages["Bob"]
```

```rust
let nums = [1, 2, 3]
let nums = nums.append(4)

let mut ages = Map.new<string, int>()
ages["Alice"] = 20
let age = ages.get("Bob") // Option<int>
```

Lisette offers `Slice<T>` and `Map<K, V>`. Map access returns `Option<V>`.

## Loops

```go
for i := 0; i < 10; i++ {
    fmt.Println(i)
}

for _, item := range items {
    process(item)
}

for {
    if done() { break }
}
```

```rust
for i in 0..10 {
  fmt.Println(i)
}

for item in items {
  process(item)
}

loop {
  if done() { break }
}
```

No C-style `for`. Use ranges (`0..10`) or `loop` for infinite loops.

## Concurrency

```go
ch := make(chan int)
go func() {
    ch <- 42
}()
v := <-ch
```

```rust
let ch = Channel.new<int>()
task { ch.send(42) }
let v = ch.receive()  // Option<int>
```

`task` instead of `go`, methods instead of operators. `receive` returns `Option<int>`. `send` returns `bool` (`false` if the channel was closed).

## Imports

```go
import "fmt"
import "net/http"
```

```rust
import "go:fmt"
import "go:net/http"
```

To import Go packages into a Lisette project, prefix with `go:`

## Strings

```go
name := "world"
msg := fmt.Sprintf("Hello, %s!", name)
```

```rust
let name = "world"
let msg = f"Hello, {name}!"
```

Format strings use `f"..."` with `{expr}` interpolation.

## Pipeline operator

```go
result := strings.ReplaceAll(strings.ToLower(strings.TrimSpace("  Hello World  ")), " ", "-")
```

```rust
let result = "  Hello World  "
  |> strings.TrimSpace
  |> strings.ToLower
  |> strings.ReplaceAll(" ", "-")
```

`|>` passes the left side as the first argument to the right side. Reads top-to-bottom instead of inside-out.

## Named numeric types

In Go, named numeric types like `time.Duration` require explicit casts for arithmetic with their underlying type:

```go
multiplier := 100
delay := time.Millisecond * time.Duration(multiplier) // cast required
```

Lisette inserts the cast automatically:

```rust
let multiplier = 100
let delay = time.Millisecond * multiplier  // works directly
```

## Unused code

In Go, unused variables and imports blocks compilation. In Lisette, they are warnings that allow compilation and can be cleaned up later.

```
  [warning] Unused variable
   ╭─[example.lis:5:7]
 4 │ fn main() {
 5 │   let x = 42
   ·       ┬
   ·       ╰── never used
 6 │   fmt.Println("hello")
   ╰────
  help: Use this variable or prefix it with an underscore: `_x`
```
