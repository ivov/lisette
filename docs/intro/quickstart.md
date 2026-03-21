# Quickstart

Prerequisites: [Rust](https://rustup.rs/) and [Go 1.25+](https://go.dev/dl/).

1. Install the CLI:

```sh
cargo install lisette
```

2. Set up your editor:

- **VS Code** (1.95): Install the [`lisette` extension](https://marketplace.visualstudio.com/items?itemName=ivov.lisette).
- **Zed** (0.226): Search for "Lisette" in the extensions panel and install.
- **Neovim** (0.11): Add to `~/.config/nvim/lua/plugins/lisette.lua`

```lua
return {
  "ivov/lisette",
  ft = "lisette",
  config = function(plugin)
    vim.opt.rtp:append(plugin.dir .. "/editors/nvim")
  end,
}
```

## Try it out

Generate a sample task manager project:

```bash
lis learn && cd learn-lisette
```

Run a few commands:

```sh
lis run -- add "Write docs" --priority high
# -> Added task 1: Write docs (high priority)

lis run -- add "Fix login bug" --tags backend
# -> Added task 2: Fix login bug (medium priority)

lis run -- add "Update deps" --priority low
# -> Added task 3: Update deps (low priority)

lis run -- done 1
# -> Completed task 1

lis run -- list
# Pending:
#  ○ 2 Fix login bug ! [backend]
#  ○ 3 Update deps
# Done:
#  ● 1 Write docs !!
```

## Explore the code

```bash
src/
├── main.lis
├── models/
│   ├── props.lis        # `Priority` and `Status` enums
│   └── task.lis         # `Task` struct with `#[json]`
├── store/
│   └── store.lis        # JSON persistence via Go interop
├── commands/
│   └── commands.lis     # CLI command handlers
└── display/
    └── display.lis      # output formatting
```

Each dir in `src/` is a module, imported by name. `main.lis` is the entry point:

```rust
import "go:fmt"
import "go:os"

import "commands"
import "display"

fn main() {
  let Some(command) = os.Args.get(1) else {
    display.print_usage()
    return
  }

  let result = match command {
    "add" => commands.add(os.Args),
    "done" => commands.done(os.Args),
    "cancel" => commands.cancel(os.Args),
    "list" => commands.list(),
    "stats" => commands.stats(),
    "watch" => commands.watch(),
    other => Err(f"unknown command: '{other}'"),
  }

  if let Err(msg) = result {
    fmt.Println(f"Error: {msg}")
    os.Exit(1)
  }
}
```

Other files contain examples of: 

- enums with data at `models/props.lis`
- `#[json]` structs at `models/task.lis`
- Go standard library interop at `store/store.lis`
- concurrency with channels at `commands/commands.lis`

To inspect the Go output, look in the `target/` dir:

```bash
target/
├── go.mod
├── main.go
├── models/
│   ├── props.go
│   └── task.go
├── store/
│   └── store.go
├── commands/
│   └── commands.go
└── display/
    └── display.go
```

## CLI

```
lis help

lisette 0.1.0 (go 1.25.5)

Usage: `lis <command>`

Commands:
    new        Create a new project
    build      Compile a project
    run        Compile and run a project
    format     Format a file or project
    check      Validate a file or project
    clean      Remove build artifacts
    learn      Generate a sample project
    doc        Explore prelude or Go stdlib
    bindgen    Generate .d.lis bindings
    lsp        Start the LSP server
    help       Print this message

Hint: Run `lis help <command>` to learn more about a command.
```

## Next steps

- Lisette's guardrails: [`safety.md`](safety.md)
- Full docs: [`reference.md`](../reference/README.md)
- If you know Go: [`coming-from-go.md`](coming-from-go.md)
- If you know Rust: [`coming-from-rust.md`](coming-from-rust.md)
