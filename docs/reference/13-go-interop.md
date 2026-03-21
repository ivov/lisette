# Go Interop

Lisette can import from the Go standard library. Third-party packages will be supported in future.

## Importing Go packages

Prefix an import with `go:` to import a Go package:

```rust
import "go:fmt"
import "go:strings"

fn main() {
  let slug = "Hello World"
    |> strings.ToLower
    |> strings.ReplaceAll(" ", "-")

  fmt.Println(slug)  // "hello-world"
}
```

The `go:` prefix distinguishes Go packages from project modules.

```rust
import "go:fmt"       // Go stdlib
import "handlers"     // project module
```

To import a Go package for its side effects only, use a blank import:

```rust
import _ "go:image/png" // registers PNG decoder
```

## Type mapping

Primitive types are identical in Lisette and Go:

- `string`, `bool`, `error`
- `int`, `int8`, `int16`, `int32`, `int64`
- `uint`, `uint8`, `uint16`, `uint32`, `uint64`
- `float32`, `float64`
- `byte`, `rune`

Compound types are different:

| Lisette        | Go                     |
| -------------- | ---------------------- |
| `Slice<T>`     | `[]T`                  |
| `Map<K, V>`    | `map[K]V`              |
| `Ref<T>`       | `*T`                   |
| `Result<T, E>` | `(T, error)` or `error`|
| `Option<T>`    | `(T, bool)`            |
| `Channel<T>`   | `chan T`               |
| `Sender<T>`    | `chan<- T`             |
| `Receiver<T>`  | `<-chan T`             |
| `VarArgs<T>`   | `...T`                 |
| `Unknown`      | `any` or `interface{}` |

### Named numeric types

Go defines types like `time.Duration` as aliases for numeric primitives: `type Duration int64`. Go's nominal type system requires explicit casts for arithmetic between these types and their underlying type.

Lisette allows arithmetic between a named numeric type and compatible types from the same family (signed integers, unsigned integers, or floats). The compiler inserts the necessary casts in the generated Go code:

```rust
import "go:time"

let multiplier = 100
let delay = time.Millisecond * multiplier  // Duration * int → Duration
let ratio = time.Minute / time.Second      // Duration / Duration → int64
```

The result type preserves the named type, with one exception: dividing two values of the same named type produces the underlying type (`T / T → U`), since a ratio is dimensionless.

Cross-family arithmetic (e.g., `Duration * float64`) remains an error.

### `Unknown`

When you call a Go function that accepts or returns a value of type `any` or `interface{}`, that value will be `Unknown` in Lisette. For example, Go's `context.Context` is declared as:

```rust
// context.d.lis
pub interface Context {
  // ...
  fn Value(key: Unknown) -> Unknown
}
```

Before you can use an `Unknown` value, call the built-in `assert_type` to narrow it down safely:

```rust
import "go:context"

let value = ctx.Value(request_id_key)    // `value` is `Unknown`
let id = assert_type<string>(value)?     // `id` is `string` or we propagate `None`
```

`assert_type` is the safe equivalent of Go's `v, ok := x.(T)`.

## Declaration files

Declaration files `.d.lis` describe Go packages in Lisette syntax.

For example, `Open` in the `os` package in the Go standard library:

```go
func Open(name string) (*File, error)
```

is declared in Lisette as:

```rust
pub fn Open(name: string) -> Result<Ref<File>, error>
```

These declarations are [bundled with the compiler](../../crates/stdlib/typedefs/), allowing you to use the Go stdlib in Lisette.

Go functions that return `(T, error)` are declared as `Result<T, E>`

```rust
import "go:os"
import "go:io"

fn read_file(path: string) -> Result<Slice<byte>, error> {
  let file = os.Open(path)?
  let bytes = io.ReadAll(file)?
  Ok(bytes)
}
```

Go's bare `error` pattern becomes `Result<(), error>`

```rust
import "go:encoding/json"

fn parse_point(data: Slice<uint8>) -> Result<Point, error> {
  let mut p = Point { x: 0, y: 0 }
  json.Unmarshal(data, &p)?
  Ok(p)
}
```

Go's `(T, bool)` pattern becomes `Option<T>`

```rust
import "go:os"

match os.LookupEnv("HOME") {
  Some(home) => fmt.Println(home),
  None => fmt.Println("HOME not set"),
}
```

Go's nullable pointers become `Option<Ref<T>>`

```rust
import "go:flag"

match flag.Lookup("verbose") {
  Some(f) => fmt.Println(f.*.Value),  // f is Ref<Flag>
  None => fmt.Println("flag not defined"),
}
```

Go functions that mutate a parameter declare it with `mut`

```rust
// sort.d.lis
pub fn Ints(mut x: Slice<int>)
```

The compiler enforces that callers pass a mutable binding:

```rust
import "go:sort"

let mut nums = [3, 1, 2]
sort.Ints(nums)   // ok

let frozen = [3, 1, 2]
sort.Ints(frozen)  // error
```

Run e.g. `lis doc go:os` to view the declaration file for a Go stdlib package or `lis doc go:os.File` to view the declaration file for a Go stdlib type.

## Panic recovery

To catch panics at runtime, Go uses `recover` in a deferred anonymous function:

```go
go func() {
    defer func() {
        if r := recover(); r != nil {
            log.Println(r)
        }
    }()
    handleConnection(conn)
}()
```

Lisette's `recover` block serves the same purpose:

```rs
task {
  let result = recover {
    handle_connection(conn)
  }

  if let Err(pv) = result {
    log.Println(pv.message())
  }
}
```

Run `lis doc PanicValue` for available methods.

<br>

<table><tr>
<td>← <a href="12-modules.md"><code>12-modules.md</code></a></td>
<td align="right"><a href="14-concurrency.md"><code>14-concurrency.md</code></a> →</td>
</tr></table>

