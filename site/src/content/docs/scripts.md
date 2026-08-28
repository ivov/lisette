---
title: "Scripts"
description: "Single-file programs, shebang, [dependencies.go]"
---

A script is a self-contained `.lis` file that does not belong to any project, i.e. not located under a `src/` dir with a `lisette.toml` manifest.

Scripts are typically used for prototypes and one-off utilities.

## Running a script

`lis run <file>` compiles the script file into a binary in a temp dir, then runs the script binary. Any argument after `<file>` is passed to the script.

```lisette
import "go:fmt"
import "go:os"

fn main() {
  fmt.Println("hello,", os.Args[1])
}
```

```sh
lis run greet.lis alice
```

A [shebang](/docs/lexemes/#shebang) makes a script directly executable:

```lisette
#!/usr/bin/env -S lis run

import "go:fmt"
import "go:os"

fn main() {
  fmt.Println("hello,", os.Args[1])
}
```

```sh
chmod +x greet.lis && ./greet.lis alice
```

Scripts support most verbs: `build`, `emit`, `check`, and `format`.

## Third-party Go modules

A script declares third-party Go dependencies in a comment block at the top of the script, above the first import:

```lisette
// [dependencies.go]
// "github.com/google/uuid" = "v1.6.0"

import "go:fmt"
import "go:github.com/google/uuid"

fn main() {
  fmt.Println(uuid.NewString())
}
```

This block is managed by `lis add --script` and `lis sync --script`.

To add a third-party Go dependency to a script:

```sh
lis add --script greet.lis google/uuid
  ✓ Added github.com/google/uuid v1.6.0
```

`lis add` writes the entry into the comment block:

```lisette
// [dependencies.go]
// "github.com/google/uuid" = "v1.6.0"
```

To reconcile the comment block against the script's imports:

```sh
lis sync --script greet.lis
```

To remove a dependency, delete its entry from the block and run `lis sync`.

## Limitations

Scripts are meant to stay small, so they are limited:

- No support for tests
- No support for redirects via `--replace` and `--path`

If a script outgrows these limits, promote it into a project: 

1. Run `lis new <project-name>`
2. Move the script file to `src/main.lis`
3. Move the comment block to `lisette.toml` without the `//` markers

