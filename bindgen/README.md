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

| Go                     | Lisette                               |
| ---------------------- | ------------------------------------- |
| `string`               | identical                             |
| `bool`                 | identical                             |
| `int`, `int64`, etc.   | identical                             |
| `uint8`, `uint16` etc. | identical                             |
| `float32`, `float64`   | identical                             |
| `any`, `interface{}`   | `Unknown`                             |
| `[]T`                  | `Slice<T>`                            |
| `[N]T`                 | `Slice<T>` with `#[go(array_return)]` |
| `map[K]V`              | `Map<K, V>`                           |
| `chan T`               | `Channel<T>`                          |
| `<-chan T`             | `Receiver<T>`                         |
| `chan<- T`             | `Sender<T>`                           |

## Contextual mappings

Other Go types map to Lisette types based on context.

### Error handling

`(T, error)` in a return type usually maps to `Result<T, error>`:

| Go return type                | Lisette return type         |
| ----------------------------- | --------------------------- |
| `(T, error)`                  | `Result<T, error>`          |
| `(T, error)` (non-exclusive)  | `Partial<T, error>`         |
| `(T1, T2, error)`             | `Result<(T1, T2), error>`   |

Some Go functions return `(T, error)` where both values may be simultaneously meaningful, such as `io.Reader.Read`. These map to `Partial<T, error>` instead of `Result<T, error>`. Bindgen detects this automatically for methods on types implementing `io.Reader`, `io.Writer`, `io.ReaderAt`, and `io.WriterAt`. Other functions can be marked manually via the `partial_result` config override.

When `error` is the sole return type, it typically maps to `Result<(), error>`. Two exceptions: functions that create errors (e.g. `errors.New`) return `error` directly, and methods that unwrap errors (e.g. `Unwrap`, `Err`, `Cause`) return `Option<error>`.

### Comma-ok pattern

In Go's `(T, bool)` return type, when `bool` signals presence, `(T, bool)` maps to `Option<T>`:

```go
func (m *Map) Load(key any) (value any, ok bool)
```

```rs
fn Load(self: Ref<Map>, key: Unknown) -> Option<Unknown>
```

When `bool` acts as a flag, `(T, bool)` is preserved as a tuple:

```go
func (m *Map) LoadAndDelete(key any) (value any, loaded bool)
```

```rs
fn LoadAndDelete(self: Ref<Map>, key: Unknown) -> (Unknown, bool)
```

### Pointers

`*T` maps to `Ref<T>` when the pointer is non-nilable or `Option<Ref<T>>` when the pointer is nilable, depending on where the pointer appears:

| Position                      | Result           |
| ----------------------------- | ---------------- |
| Pointer in function parameter | `Ref<T>`         |
| Pointer in struct field       | `Option<Ref<T>>` |
| Pointer in container element  | `Option<Ref<T>>` |

Pointers in function return types are typically non-nilable, unless a [heuristic](internal/convert/nilcheck.go) says otherwise.

```go
func Open(name string) (*File, error)  // non-nil on success (typical case)
func NewFile(fd uintptr, name string) *File  // nil on invalid fd (heuristic)
```

```rs
pub fn Open(name: string) -> Result<Ref<File>, error>
pub fn NewFile(fd: uint, name: string) -> Option<Ref<File>>
```

### Value enums

Groups of iota-based constants sharing a named type map to value enums:

```go
type Month int
const (
    January Month = 1 + iota
    February
    // ...
)
```

```rs
pub enum Month: int {
  January = 1,
  February = 2,
  // ...
}
```

Value enums are a `.d.lis`-only construct for representing iota-based constant groups.

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
    },

    // Override type mapping decisions
    "types": {
      // Turn `Ref<T>` into `Option<Ref<T>>`
      // e.g. `os.NewFile` returns `Option<Ref<File>>`
      "nilable_return": {
        "os": ["NewFile"],
      },

      // Turn `Option<Ref<T>>` into `Ref<T>`
      // e.g. `reflect.TypeOf` returns `Ref<Type>`
      "non_nilable_return": {
        "reflect": ["TypeOf"],
      },

      // Return `error` directly instead of `Result<(), error>`
      // e.g. `errors.New` returns `error`
      "direct_error": {
        "errors": ["New"],
      },

      // Return `Option<error>` instead of `Result<(), error>`
      // e.g. `errors.Unwrap` returns `Option<error>`
      "nilable_error": {
        "errors": ["Unwrap"],
      },

      // Map `(T, error)` to `Partial<T, error>` instead of `Result<T, error>`
      // for Go functions where both return values are simultaneously meaningful.
      // e.g. `io.ReadAtLeast` returns `Partial<int, error>`
      "partial_result": {
        "io": ["ReadAtLeast", "ReadFull"],
      },

      // Return `Never` instead of `()` for functions that do not return normally
      // e.g. `os.Exit` returns `Never`
      "never_return": {
        "os": ["Exit"],
      },

      // Keep `(T, bool)` as tuple instead of `Option<T>`
      // e.g. `math/big.Rat.Float32` returns `(float32, bool)`
      "bool_as_flag": {
        "math/big": ["Rat.Float32", "Rat.Float64"],
      },

      // Mark parameter as mutable
      // e.g. `buf` in `io.CopyBuffer(dst, src, mut buf)` is mutable
      "mutates_param": {
        "io": {
          "CopyBuffer": ["buf"],
        },
      },
    },
  },
}
```
