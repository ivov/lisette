# Modules

A module is a directory. All `.lis` files in a directory belong to the same module, and the directory name is the module name.

```
my_project/
├── lisette.toml
└── src/
    ├── main.lis
    ├── models/
    │   ├── user.lis
    │   └── post.lis
    └── routes/
        ├── api.lis
        └── admin/
            └── dashboard.lis
```

In this project:
- `src/` contains the entry point `main.lis`
- `models/` is a module named `models`
- `routes/` is a module named `routes`
- `routes/admin/` is a module named `routes/admin`

Definitions in `user.lis` and `post.lis` are part of the same `models` module.

## Imports

Import a module by path:

```rust
import "models"
import "routes"
```

The path is relative to the project root. For nested modules:

```rust
import "routes/admin"
```

Imported definitions are namespaced under the module name:

```rust
import "models"

fn main() {
  let u = models.User { name: "Alice" }
  models.save(u)
}
```

Use an alias to rename an imported module:

```rust
import m "models"

fn main() {
  let u = m.User { name: "Alice" }
}
```

Circular imports are disallowed.

## Visibility

By default, definitions are private to their module. All files in a module are visible to each other. Use `pub` to make them visible to other modules:

```rust
// in models/user.lis

pub struct User {
  pub name: string,
  pub email: string,
}

pub fn save(user: User) {
  // ...
}

fn validate(user: User) -> bool {
  // private, only accessible inside the `models` module
}
```

A struct and its fields can be marked `pub` independently:

```rust
// in config/mod.lis
pub struct Config {
  pub debug: bool,
  secret_key: string,
}

// in main.lis
import "config"

fn handle(c: config.Config) {
  c.secret_key
}
```

Accessing a private field from another module is an error:

```
error: Private field
 5 │     c.secret_key
   ·       ─────┬────
   ·            ╰── private
   ╰────
  help: Cannot access private field `secret_key` of struct `config.Config`
```

## The prelude

Lisette's prelude is a set of definitions that are always available in every file without an import.

- Primitive types: `int`, `string`, `bool`, `float64`, etc.
- Common types: `Option`, `Result`, `Slice`, `Map`
- Enum variants: `Some`, `None`, `Ok`, `Err`
- among others

Run `lis doc` to view all prelude definitions.

<br>

<table><tr>
<td>← <a href="11-interfaces.md"><code>11-interfaces.md</code></a></td>
<td align="right"><a href="13-go-interop.md"><code>13-go-interop.md</code></a> →</td>
</tr></table>
