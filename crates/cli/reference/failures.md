---
title: "Failures"
description: "? propagation, error context, try blocks, custom errors, partial results, no unwrap, panic recovery"
---

## `?` for propagation

On a `Result`, `?` unwraps `Ok` on success, and force-returns `Err` on failure:

```lisette
fn read_config(path: string) -> Result<Config, error> {
  // !callout-center[/\?/] on success, `bytes` is the value inside `Ok`\non failure, `read_config` returns `Err`
  let bytes = os.ReadFile(path)?
  parse_config(bytes)
}
```

Equivalent without `?`:

```lisette
fn read_config(path: string) -> Result<Config, error> {
  let bytes = match os.ReadFile(path) {
    Ok(contents) => contents,
    Err(failure) => return Err(failure),
  }
  parse_config(bytes)
}
```

On an `Option`, `?` unwraps `Some` when present, and force-returns `None` when absent:

```lisette
fn get_name(id: int) -> Option<string> {
  // !callout-center[/\?/] when present, `user` is the value inside `Some`\nwhen absent, `get_name` returns `None`
  let user = users.get(id)?
  Some(user.name)
}
```

Equivalent without `?`:

```lisette
fn get_name(id: int) -> Option<string> {
  let user = match users.get(id) {
    Some(found) => found,
    None => return None,
  }
  Some(user.name)
}
```

## Attaching context to errors

`?` propagates an error unchanged. To record where it passed through, `wrap_err()` prepends a message to the error while keeping the original as the cause:

```lisette
fn read_config(path: string) -> Result<Config, error> {
  let bytes = os.ReadFile(path).wrap_err("reading config file")?
  parse_config(bytes)
}
```

An `Ok` passes through untouched. If `os.ReadFile` fails, `?` propagates the wrapped error, which reads `reading config file: open /etc/app.conf: no such file or directory`. The original is preserved, so `errors.Is` and `errors.As` still match against it.

## `try` blocks

A `try` block groups fallible calls and evaluates to a `Result`. Inside it, `?` returns from the block rather than from the function:

```lisette
fn load_config() -> Config {
  let result = try {
    // !callout-right failure propagates to `result`
    let path = env.get("CONFIG_PATH")?
    let file = fs.read(path)?
    parse_toml(file)?
  }

  match result {
    Ok(config) => config,
    Err(_) => Config.default(),
  }
}
```

## Defining your own errors

Any type with an `Error()` method can be returned as an error:

```lisette
struct ValidationError {
  field: string,
  message: string,
}

impl ValidationError {
  fn Error(self) -> string {
    f"{self.field}: {self.message}"
  }
}

fn validate(input: Input) -> Result<Input, ValidationError> {
  if input.name == "" {
    return Err(ValidationError { field: "name", message: "required" })
  }
  Ok(input)
}
```

It then propagates with `?` into any function returning [`error`](/docs/prelude/types/#error-interface):

```lisette
fn process(input: Input) -> Result<Input, error> {
  // !callout-right `ValidationError` satisfies `error`
  let valid = validate(input)?
  Ok(valid)
}
```

## Partial results

Where a Go function returns a value alongside an error, the result is a [`Partial<T, E>`](/docs/prelude/partial/). Handle its three cases with `match`:

```lisette
match reader.Read(buf) {
  Partial.Ok(n) => process(buf[..n]),
  Partial.Err(err) => return Err(err),
  // !callout-right read bytes, then reached EOF
  Partial.Both(n, err) => {
    process(buf[..n])
    if errors.Is(err, io.EOF) { return Ok(()) }
    return Err(err)
  },
}
```

An `error` is an interface value, so `==` is refused on it. Compare with `errors.Is`, which also matches an error wrapped by `wrap_err()`.

## No `unwrap()`

Lisette omits Rust's [`unwrap()`](https://doc.rust-lang.org/std/result/enum.Result.html#method.unwrap). To unwrap a value, use:

- [`?`](#-for-propagation) to propagate failure
- [`match`](/docs/pattern-matching/#match-expressions) to handle every case
- [`let else`](/docs/pattern-matching/#let-else) for early exit
- [`unwrap_or()`](/docs/prelude/result/#resultunwrap_or) with a default

## Panic recovery

To catch panics at runtime, Go uses [`recover()`](https://go.dev/blog/defer-panic-and-recover) in a deferred anonymous function:

```go file="recover.go"
go func() {
    defer func() {
        if r := recover(); r != nil {
            log.Println(r)
        }
    }()
    handleConnection(conn)
}()
```

Lisette's `recover` block serves the same purpose, yielding a [`PanicValue`](/docs/prelude/types/#panicvalue) that carries what the panic passed:

```lisette file="recover.lis"
let result = recover {
  handle_connection(conn)
}

if let Err(pv) = result {
  log.Println(pv.message())
}
```
