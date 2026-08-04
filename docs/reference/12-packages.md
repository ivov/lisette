# Packages

A package is a directory. All `.lis` files in a directory belong to the same package, and the directory name is the package name.

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
- `models/` is a package named `models`
- `routes/` is a package named `routes`
- `routes/admin/` is a package named `routes/admin`

Definitions in `user.lis` and `post.lis` are part of the same `models` package.

## Imports

Import a package by path:

```rust
import "models"
import "routes"
```

Imports must come before every other top-level item in a file. Only comments may precede them.

The path is relative to the project root. For nested packages:

```rust
import "routes/admin"
```

Imported definitions are namespaced under the package name:

```rust
import "models"

fn main() {
  let u = models.User { name: "Alice" }
  models.save(u)
}
```

Use an alias to rename an imported package:

```rust
import m "models"

fn main() {
  let u = m.User { name: "Alice" }
}
```

Circular imports are disallowed.

## Visibility

By default, definitions are private to their package. All files in a package are visible to each other. Use `pub` to make them visible to other packages:

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
  // private, only accessible inside the `models` package
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

Accessing a private field from another package is an error:

```
error: Private field
 5 │     c.secret_key
   ·       ─────┬────
   ·            ╰── private
   ╰────
  help: Cannot access private field `secret_key` of struct `config.Config`
```

## Library projects

A project with `src/main.lis` is a binary: `lis build` compiles the file tree under `src/` into an executable at `target/.lisette/bin/`.

A project without `src/main.lis` is a library: `lis build` turns the packages under `src/` into Go packages at `target/` and files at `src/` become the module's root package. `src/internal/` dir stays private, as in Go. Set `name` in `lisette.toml` to the Go module path that consumers will import.

```sh
geo/
├── lisette.toml        # name = "github.com/you/geo"
└── src/
    ├── geo.lis         # emits: target/geo.go, package geo
    └── shapes/
        └── shapes.lis  # emits: target/shapes/shapes.go, package shapes
```

To externally test a library's root package, see [Testing a library root](16-testing.md#testing-a-library-root).

## The prelude

Lisette's prelude is a set of definitions that are always available in every file without an import.

- `int`, `string`, `bool`, `float64`, etc.
- `Option`, `Result`, `Array`, `Slice`, `Map`
- `Some`, `None`, `Ok`, `Err`
- among others

Run `lis doc` to view all prelude definitions.

<br>

<table><tr>
<td>← <a href="11-interfaces.md"><code>11-interfaces.md</code></a></td>
<td align="right"><a href="13-go-interop.md"><code>13-go-interop.md</code></a> →</td>
</tr></table>
