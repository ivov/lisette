---
title: "Third-party Go modules"
description: "Adding, importing, tidying and removing third-party Go modules"
---

## Adding a module

To add a third-party module as a dependency to your project:

```sh
# !callout-right latest version
lis add google/uuid
# !callout-right exact version
lis add google/uuid@v1.6.0
# !callout-right exact commit hash or branch
lis add google/uuid@2d3c2a9
# !callout-right full path for a non-GitHub host
lis add go.uber.org/zap
```

Example output:

```plaintext
✓ Added go.uber.org/zap v1.28.0
  └─ go.uber.org/multierr v1.10.0
```

`lis add` will:

1. download the Go module to `~/go/pkg/mod`,
2. generate [typedefs](/docs/typedefs/) at `target/.lisette/typedefs`, and
3. declare the module and its transitives in the `lisette.toml` manifest.

```toml
[dependencies.go]
"go.uber.org/zap" = "v1.28.0"
"go.uber.org/multierr" = { version = "v1.10.0", via = ["go.uber.org/zap"] }
```

`lis add` does not generate every typedef up front. Typedefs for packages no import reaches are generated on demand, when you write the import.

## Importing a module

To import a third-party module:

```lisette
import "go:go.uber.org/zap"

fn main() {
  let logger = zap.NewExample()
  logger.Info("hello")
}
```

The imported module must be listed in your project manifest.

## Tidying the manifest

To tidy up the manifest:

```sh
lis sync
```

This reconciles `lisette.toml` against state on disk. Run `lis sync` after removing imports, deleting source files, or pulling new code.

## Removing a module

To remove a dependency:

- Delete the entry from `lisette.toml`
- Run `lis sync`
