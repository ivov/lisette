# Error Handling

Lisette uses `Result<T, E>` for operations that may fail and `Option<T>` for values that may be absent. See [`06-structs-and-enums.md`](06-structs-and-enums.md)

## The `?` operator

The `?` operator exits early on error.

When applied to `Result`, `?` unwraps `Ok` or forces the function to return `Err`:

```rust
fn read_config(path: string) -> Result<Config, error> {
  let file = os.Open(path)?
  let bytes = io.ReadAll(file)?
  parse_config(bytes)
}
```

Without `?`:

```rust
fn read_config(path: string) -> Result<Config, error> {
  let file = match os.Open(path) {
    Ok(f) => f,
    Err(e) => return Err(e),
  }
  let bytes = match io.ReadAll(file) {
    Ok(b) => b,
    Err(e) => return Err(e),
  }
  parse_config(bytes)
}
```

When applied to `Option`, `?` unwraps `Some` or forces the function to return `None`:

```rust
fn get_name(id: int) -> Option<string> {
  let user = users.get(id)?
  Some(user.name)
}
```

Without `?`:

```rust
fn get_name(id: int) -> Option<string> {
  let user = match users.get(id) {
    Some(u) => u,
    None => return None,
  }
  Some(user.name)
}
```

## `try` blocks

The `?` operator only works in functions that return `Result` or `Option`. 

For functions that return `T`, a `try` block creates a dedicated scope for `?`, so that the enclosing function is not required to include `Result` or `Option` in its return type.

```rust
fn load_config() -> Config {
  let result = try {
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

## Custom error types

If a type implements the `error` interface, it can be used as an error:

```rust
interface error {
  fn Error() -> string
}
```

Define a custom error:

```rust
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

📚 See [`10-methods.md`](10-methods.md)

## No `unwrap()`

Lisette deliberately omits `unwrap()`. To extract a value, use:

- `?` to propagate
- `match` to handle both cases
- `let else` for early exit (see [`08-pattern-matching.md`](08-pattern-matching.md))
- `unwrap_or` with a default

This prevents panics from unhandled `None` or `Err` values.

<br>

<table><tr>
<td>← <a href="08-pattern-matching.md"><code>08-pattern-matching.md</code></a></td>
<td align="right"><a href="10-methods.md"><code>10-methods.md</code></a> →</td>
</tr></table>
