# Attributes

Attributes attach metadata to structs and fields, typically for serialization.

## Serialization

Add `#[json]` to a struct to generate Go JSON struct tags for all fields:

```rust
#[json]
struct User {
  name: string,
  age: int,
  active: bool,
}
```

Generated Go:

```go
type User struct {
  Name   string `json:"name"`
  Age    int    `json:"age"`
  Active bool   `json:"active"`
}
```

Supported serialization attributes: `json`, `xml`, `yaml`, `toml`, `db`, `bson`, `mapstructure`, `msgpack`.

Serialization attributes accept options:

| Option       | Effect                           |
| ------------ | -------------------------------- |
| `omitempty`  | Omit field if empty              |
| `!omitempty` | Include field if empty           |
| `skip`       | Exclude field                    |
| `snake_case` | Convert field name to snake_case |
| `camel_case` | Convert field name to camelCase  |
| `string`     | Encode numbers as strings        |

```rust
#[json]
struct Config {
  #[json(omitempty)]
  timeout: Option<int>,

  #[json(skip)]
  internal_id: int,

  #[json(string)]
  large_number: int64,
}
```

Or override the serialized field name:

```rust
#[json]
struct User {
  #[json("user_id")]
  id: int,

  #[json("displayName")]
  name: string,
}
```

Struct-level options apply to all fields; field-level options override:

```rust
#[json(snake_case)]
struct UserProfile {
  userName: string,        // "user_name"

  createdAt: int,          // "created_at"

  #[json("userID")]
  uniqueId: int,           // "userID" (override)
}
```

Multiple attributes are supported:

```rust
#[json]
#[db]
struct User {
  #[json("userName")]
  #[db("user_name")]
  name: string,
}
```

## Custom tags

For custom tags, use `#[tag]`

```rust
#[json]
struct Input {
  #[tag("validate", "required")]
  email: string,
}
```

Generated Go:

```go
type Input struct {
  Email string `json:"email" validate:"required"`
}
```

For more complex tags, use a backticked string:

```rust
#[json]
struct User {
  #[tag(`validate:"required,email" gorm:"unique"`)]
  email: string,
}
```

Generated Go:

```go
type User struct {
  Email string `json:"email" validate:"required,email" gorm:"unique"`
}
```

## Lint suppression

Use `#[allow]` on a function to suppress an unused expression lint on its call sites. 

Lint rules currently suppressible: `unused_result`, `unused_option`, `unused_literal`, `unused_value`.

```rust
import "go:os"

#[allow(unused_result)]
fn warm_cache(path: string) {
  os.ReadFile(path)  // preload file, ignore contents
}
```

<br>

<table><tr>
<td>← <a href="14-concurrency.md"><code>14-concurrency.md</code></a></td>
</tr></table>
