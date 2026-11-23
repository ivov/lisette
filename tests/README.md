# Tests

Tests are grouped by prefix, from fast to slow.

| Prefix | Checks | Run | CI |
|---|---|---|---|
| `fast_` | compiler (and LSP) as a lib | `just test-fast` | every PR |
| `cli_` | `lis` CLI commands | `just test-cli` | every PR |
| `go_` | Lis-emitted Go via Go toolchain | `just test-go` | every PR |
| `fuzz_` | random changes to test programs | `just test-fuzz` | nightly |

Each crate also has unit tests in its source files.

## Groups

`fast_`

These tests call the compiler or the language server as a Rust library, inside the test process.

- `fast_suite` tests each compiler stage: lexer, parser, type checker, lints, formatter and emit. A test checks a result such as an error code, or compares the output with a saved snapshot.
- `fast_lsp` checks the answers of the language server, e.g. hover, completion and diagnostics.
- `fast_release` checks that a published release installs with matching crates and knows the Go standard library on every platform.

`cli_`

These tests run the `lis` binary on small projects, as a user does.

- `cli_language` runs `lis test` on Lisette tests of each language area, in `cli_language/project/`.
- `cli_run` checks that `lis check`, `lis build` and `lis run` work on many small programs. It also checks that when `lis run` gets a stop signal, the program stops too.
- `cli_cross` checks builds for other platforms with `--target`.
- `cli_library` checks library projects, and that a Go program can import a library.
- `cli_go_dependencies` checks that `lis add` and `lis sync` record the right Go modules.
- `cli_external_tests` checks that `lis test` runs external and internal tests and reports their results.
- `cli_learn` checks that the tutorial project from `lis learn` passes `lis test`.
- `cli_lsp` checks that `lis lsp` does not run `go` while you type, and can run it when you save.

`go_`

These tests check Lis-emitted Go via the Go toolchain: `go build`, `go vet`, `go test` or `go/types`.

- `go_emit` checks that the Go of the `fast_suite` programs builds and passes its tests. It also checks generated programs with type conversions, e.g. a struct where Go expects an `error`. A program with no test, or one that cannot run, only needs to type-check. It also checks equivalent copies of the emit snapshot programs, e.g. with extra parens or type aliases.
- `go_rejections` checks with `go/types` that `lis check` rejects exactly the programs that Go rejects, for small programs about constants, operators, interfaces and embedded types. When `lis check` rejects a program, Go checks a Go version of the same program.

`fuzz_`

These tests change programs at random for a fixed time.

- `fuzz_emit` swaps one to three expressions of an emit snapshot program for random expressions of the same kind, e.g. one call for another. When `lis check` accepts the result, its Go must pass `go build`.

## Type-check-only and failing lists

The `go_` and `fuzz_` tests use lists of program names, one per line, with a `#` note.

- `go_emit/typecheck_only.txt` lists emit snapshot programs that cannot run, e.g. programs that block forever. `go_emit` only type-checks them, and `fuzz_emit` does not use them.
- Each `failing*.txt` lists programs that fail because of an open bug. When you fix the bug, delete them, or the test fails.
