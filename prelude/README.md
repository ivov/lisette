# lisette/prelude

Types for [Lisette](https://lisette.run), a language inspired by Rust that compiles to Go.

| Type            | Description                                                                                                         |
| --------------- | ------------------------------------------------------------------------------------------------------------------- |
| `Option[T]`     | A value that is either present (`Some`) or absent (`None`). Replaces nilable pointers and comma-ok patterns.        |
| `Result[T, E]`  | A value that is either a success (`Ok`) or a failure (`Err`). Replaces `(T, error)` return patterns.                |
| `Partial[T, E]` | A result that may carry both a value and an error (`Ok`, `Err`, or `Both`). For non-exclusive `(T, error)` returns. |

## Usage from Go

```go
import lisette "github.com/ivov/lisette/prelude"

// Option
opt := lisette.MakeOptionSome(42)
opt.Is_some() // true
opt.Unwrap_or(0) // 42
none := lisette.MakeOptionNone[int]()
none.Unwrap_or(0) // 0

// Result
res := lisette.MakeResultOk[int, string](42)
res.Is_ok() // true
res.Unwrap_or(0) // 42
```

## Learn more

By visiting the repository: [`github.com/ivov/lisette`](https://github.com/ivov/lisette)
