# bindgen

Bindings generator for [Lisette](https://lisette.run), a language inspired by Rust that compiles to Go.

> [!IMPORTANT]
> Do **not** install or use this tool directly. It is intended for internal use by the Lisette compiler.

## How it works

Lisette's bindings generator

- reads the public API for one or more Go packages,
- maps those Go symbols to Lisette types, and
- emits `.d.lis` type definition files.

Go symbols here means Go functions, types, methods, constants, variables, etc.

For example:

```go
func Atoi(s string) (int, error)
```

maps to

```rs
pub fn Atoi(s: string) -> Result<int, error>
```

in `strconv.d.lis`.

## Usage

The following commands are for local dev only. First build the binary from the Lisette project root:

```bash
cd bindgen && just build
```

To generate bindings for the Go stdlib:

```bash
bindgen/bin/bindgen stdlib \
  -config bindgen/bindgen.stdlib.json \
  -outdir crates/stdlib/typedefs
```

To generate bindings for a third-party Go dependency:

```bash
bindgen/bin/bindgen pkg github.com/gorilla/mux \
  -config my-config.json
```

## Simple mappings

Some Go types map 1:1 to Lisette types.

| Go                     | Lisette         |
| ---------------------- | ----------------|
| `string`               | identical       |
| `bool`                 | identical       |
| `int`, `int64`, etc.   | identical       |
| `uint8`, `uint16` etc. | identical       |
| `float32`, `float64`   | identical       |
| `any`, `interface{}`   | `Unknown`       |
| `[]T`                  | `Slice<T>`      |
| `[N]T`                 | `Array<T, N>`   |
| `map[K]V`              | `Map<K, V>`     |
| `chan T`               | `Channel<T>`    |
| `<-chan T`             | `Receiver<T>`   |
| `chan<- T`             | `Sender<T>`     |

## Contextual mappings

Other Go types map to Lisette types based on context.

### Error handling

`(T, error)` in a return type usually maps to `Result<T, error>`:

| Go return type                | Lisette return type         |
| ----------------------------- | --------------------------- |
| `(T, error)`                  | `Result<T, error>`          |
| `(T, error)` (non-exclusive)  | `Partial<T, error>`         |
| `(T1, T2, error)`             | `Result<(T1, T2), error>`   |

Some Go functions return `(T, error)` where both values may be simultaneously meaningful, such as `io.Reader.Read`. These map to `Partial<T, error>` instead of `Result<T, error>`. Bindgen detects this automatically for methods on types implementing `io.Reader`, `io.Writer`, `io.ReaderAt`, `io.WriterAt`, `io.StringWriter`, `io.ReaderFrom`, and `io.WriterTo`. Other functions can be marked manually via the `partial_result` config override.

When `error` is the sole return type, it typically maps to `Result<(), error>`. Two exceptions: functions that create errors (e.g. `errors.New`) return `error` directly, and methods that unwrap errors (e.g. `Unwrap`, `Err`, `Cause`) return `Option<error>`.

### Comma-ok pattern

In Go's `(T, bool)` return type, when `bool` signals presence, `(T, bool)` maps to `Option<T>`. Bindgen reads the function body to check this: each `return` that gives `false` must also give the zero value of `T`. When `T` can be nil, each `return` that can give `true` must also give a value that is not nil, because `Some` cannot hold nil. Otherwise the result maps to `(Option<T>, bool)`.

```go
func (m *Map) Load(key any) (value any, ok bool)
```

```rs
fn Load(self: mut Ref<Map>, key: Unknown) -> Option<Unknown>
```

When bindgen cannot prove it, `(T, bool)` is preserved as a tuple, so no value is lost:

```go
func (m *Map) LoadOrStore(key, value any) (actual any, loaded bool)
```

```rs
fn LoadOrStore(self: mut Ref<Map>, key: Unknown, value: Unknown) -> (Unknown, bool)
```

Interface methods and function types have no body to read, so they stay tuples unless the `bool_as_presence` override lists them. A method that implements an interface method from its own package or a direct import uses the same mapping, so its type still implements the interface.

### Pointers

`*T` maps to `Ref<T>` when the pointer is non-nilable or `Option<Ref<T>>` when the pointer is nilable, depending on where the pointer appears:

| Position                      | Result           |
| ----------------------------- | ---------------- |
| Pointer in function parameter | `Ref<T>`         |
| Pointer in struct field       | `Option<Ref<T>>` |
| Pointer in container element  | `Option<Ref<T>>` |

For pointer and interface return types, bindgen runs an [SSA nilability analysis](internal/convert/nilness.go) over the whole loaded program: a return proven non-nil on every path emits `Ref<T>`, a witnessed nil path emits `Option<Ref<T>>`, and an inconclusive body emits `Option<Ref<T>>`, unless a config override pins it.

```go
func Open(name string) (*File, error)  // non-nil on success (typical case)
func NewFile(fd uintptr, name string) *File  // nil on invalid fd (proven nil path)
```

```rs
pub fn Open(name: string) -> Result<Ref<File>, error>
pub fn NewFile(fd: uint, name: string) -> Option<Ref<File>>
```

A `(T, error)` return presumes `T` non-nil on success, but when the analysis witnesses a return site where both values are nil, the payload demotes to `Option`:

```go
func (r *Reader) Next() (*Entry, error)  // returns nil, nil at end of section
```

```rs
fn Next(self: Ref<Reader>) -> Result<Option<Ref<Entry>>, error>
```

The same applies per element in plain tuples, so `encoding/pem.Decode` emits `(Option<Ref<Block>>, Slice<byte>)`.

### Named primitive types

A Go `type X <primitive>` declaration maps to a single-field tuple struct.

```go
type Month int
const (
    January Month = 1 + iota
    February
    // ...
)
```

```rs
pub struct Month(int)

pub const January: Month = 1
pub const February: Month = 2
// ...
```


### Opaque types

Structs with no exported fields emit as opaque type definitions:

```rs
pub type Mutex
```

## Config file

Bindgen accepts a config file with per-package overrides:

```jsonc
{
  "overrides": {
    // Suppress specific lint warnings
    "lints": {
      "allow_unused_result": {
        "fmt": ["Print", "Printf", "Println"],
      },

      // Suppress unused_value on fluent registration APIs whose return is a
      // shared singleton callers idiomatically discard.
      // e.g. `web.Get/Post` in beego returns `*HttpServer` for chaining
      "allow_unused_value": {
        "github.com/beego/beego/v2/server/web": ["Get", "Post"],
      },
    },

    // Override type mapping decisions
    "types": {
      // Turn `Ref<T>` into `Option<Ref<T>>` when the analysis cannot see
      // the nil path itself. On a `(T, error)` result, only `T` becomes
      // `Option`.
      "nilable_return": {
        "example.com/pkg": ["NewHandle"],
      },

      // Turn `Option<Ref<T>>` into `Ref<T>` for API invariants the analysis
      // cannot prove
      // e.g. `sync.OnceValue` never returns a nil function value
      "non_nilable_return": {
        "sync": ["OnceValue"],
      },

      // Return `error` directly instead of `Result<(), error>`
      // e.g. `errors.New` returns `error`
      "direct_error": {
        "errors": ["New"],
      },

      // Return `Option<error>` instead of `Result<(), error>`, or type an
      // error field as `Option<error>`
      // e.g. `errors.Unwrap` returns `Option<error>`, and `exec.Cmd.Err` is nil
      // when the lookup succeeds
      "nilable_error": {
        "errors": ["Unwrap"],
        "os/exec": ["Cmd.Err"],
      },

      // Map `(T, error)` to `Partial<T, error>` instead of `Result<T, error>`
      // for Go functions where both return values are simultaneously meaningful.
      // e.g. `io.ReadAtLeast` returns `Partial<int, error>`
      "partial_result": {
        "io": ["ReadAtLeast", "ReadFull"],
      },

      // Keep `(T, bool)` as tuple instead of `Option<T>`
      // e.g. `math/big.Rat.Float32` returns `(float32, bool)`
      "bool_as_flag": {
        "math/big": ["Rat.Float32", "Rat.Float64"],
      },

      // Turn `(T, bool)` into `Option<T>` when bindgen cannot prove
      // that `bool` signals presence, or that a true `bool` gives a value
      // that is not nil
      // e.g. `os.LookupEnv` returns `Option<string>`
      // e.g. `iter.Pull` returns a `next` function that returns `Option<V>`
      "bool_as_presence": {
        "os": ["LookupEnv"],
        "iter": ["Pull"],
      },

      // Mark parameter as mutable
      // e.g. `buf` in `io.CopyBuffer(dst, src, mut buf)` is mutable
      "mutates_param": {
        "io": {
          "CopyBuffer": ["buf"],
        },
      },

      // Mark a result as one callers write, where no aliasing fact can be
      // stated. Prefer `returns_view_of` when the aliasing fact is known.
      // e.g. `w.Header().Set(...)` needs `mut net/http.Header`
      "writable_return": {
        "net/http": ["ResponseWriter.Header"],
      },

      // Force `Ref<T>` parameter to `Option<Ref<T>>` when inference did not
      // e.g. `rp` in `mongo.Client.Ping(ctx, rp)` accepts nil
      "nilable_param": {
        "go.mongodb.org/mongo-driver/v2/mongo": {
          "Client.Ping": ["rp"],
        },
      },

      // Force a parameter of a function type to `Option`, by name or position,
      // when Go passes nil to it. Key by the type, field or "Function.param"
      // e.g. `fs.WalkDirFunc` gets a nil `d` and `err`, and `ast.Inspect`
      // calls `f(nil)`
      "nilable_callback_param": {
        "io/fs": {
          "WalkDirFunc": ["d", "err"],
        },
        "go/ast": {
          "Inspect.f": ["0"],
        },
      },

      // The same as `nilable_return` for the result of a function type,
      // keyed by the symbol that holds the type
      // e.g. the `Transport.Proxy` field returns a nil URL and a nil error
      // for "no proxy"
      "nilable_callback_return": {
        "net/http": ["Transport.Proxy"],
      },

      // Keep `Ref<T>` parameter when inference wrongly proved it accepts nil
      "non_nilable_param": {},

      // Allow constructing a type at its Go zero value, verified against Go's docs.
      // Applies to types with no visible fields, which are refused by default
      // e.g. `sync.Mutex` documents its zero value as an unlocked mutex
      "zero_safe": {
        "sync": ["Mutex", "WaitGroup"],
      },

      // Deny constructing a struct by literal, forcing its Go constructor.
      // Applies to structs with visible fields, which are admitted by default.
      // Use for types whose zero value panics, verified against Go's docs or
      // by probing the zero value
      // e.g. a zero `csv.Writer` panics on `Write` (nil internal writer)
      "zero_unsafe": {
        "encoding/csv": ["Reader", "Writer"],
      },
    },
  },
}
```
