# Learn Lisette

A CLI task manager that demonstrates how a real Lisette project is structured.

```bash
lis run -- add "Fix login timeout" --priority high --tags bug,auth
lis run -- add "Update dependencies" --tags maintenance
lis run -- add "Write blog post" --priority low
lis run -- done 1
lis run -- cancel 3 "moved to wiki"
lis run -- list
lis run -- stats
lis run -- watch
```

Tasks are saved to `tasks.json` in the working directory.

## Project structure

```bash
src/
  main.lis              # entry point
  models/
    props.lis           # `Priority` and `Status` enums
    task.lis            # `Task` struct
  store/
    store.lis           # JSON persistence
  commands/
    commands.lis        # CLI commands
  display/
    display.lis         # output formatting
```

Each directory under `src/` is a module, imported by its directory name (e.g. `import "models"`). Files within a module share the same namespace, so `props.lis` and `task.lis` both contribute to the `models` module.

Language features shown:

| Feature                                           | Where to look                              |
| ------------------------------------------------- | ------------------------------------------ |
| Enums, `#[json]` serialization                    | `models/props.lis`                         |
| Structs, impl blocks, associated functions        | `models/task.lis`                          |
| Pattern matching (`match`, `let...else`)          | `main.lis`, `store/store.lis`              |
| Error handling (`Result`, `?`)                    | `commands/commands.lis`                    |
| Closures, slice methods (`filter`, `map`, `fold`) | `store/store.lis`, `commands/commands.lis` |
| Go interop (`go:` imports, `as` casting)          | `store/store.lis`                          |
| Mutability (`let mut`, `&`)                       | `commands/commands.lis`                    |
| Concurrency (`task`, channels)                    | `commands/commands.lis` (`watch`)          |
| F-strings                                         | throughout                                 |

## Next steps

Try modifying the project:

- Add a `remove` command that deletes a task by ID
- Add a `--status` filter flag to the `list` command
- Add a `due_date` field to `Task` using `Option<string>`
