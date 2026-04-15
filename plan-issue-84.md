# Plan: Auto-regenerate Go typedefs after lis upgrade (issue #84)

## Problem

`~/.lisette/cache/typedefs/` is keyed by lis version (`lis@v{CARGO_PKG_VERSION}`, see `crates/deps/src/lib.rs:22-25`). Upgrading lis points every project at an empty cache dir, so every Go import errors with `missing_go_typedef` even though `lisette.toml` still pins exact versions. The error also recommends `lis sync`, which does not exist.

User expectation (from issue): the next invocation after upgrade should just work.

## Design

Version-keyed cache stays — it is the right design (same lis → same bindgen → deterministic output, per `lisette-specs/lis-imports-go.md`). Fix the two user-visible symptoms:

1. An empty per-version cache when `lisette.toml` has `[dependencies.go]` should trigger a silent, one-shot regeneration on the next command, not an error. All inputs are already pinned; lis has everything it needs.
2. When an individual regeneration genuinely fails and `missing_go_typedef` does fire, its hint must point at a command that exists.

Typedef resolution itself stays pure local-file lookup. The regeneration hook runs *before* resolution, not from inside it.

## Scope

### 1. Fix the dead-end hint

`crates/diagnostics/src/module_graph.rs:75-78` — replace the `lis sync` suggestion with `lis add <module>@<version>`. `lis add` against an already-pinned version is idempotent and refills the cache (confirmed against current `GoWorkspace::reconcile` behavior). Closes bug #10 in `bugs.md:447`.

New wording (covers CLI and LSP in one line — see note on LSP below):

```
Package `{go_pkg}` is declared via `{module}` {version} but no typedef
was found. Run `lis check` to regenerate all typedefs, or
`lis add {module}@{version}` to regenerate this one.
```

The `lis check` half is what makes this the LSP fix: when a user opens a file post-upgrade, every broken Go import shows this hint on hover, and one terminal command fixes all N deps at once. The `lis add` half remains the targeted remediation surfaced by the CLI after per-module regen failure.

### 2. New module: `crates/cli/src/go_regen.rs`

Single public entry point:

```rust
pub fn ensure_typedefs_for_current_version(
    project_ctx: &ProjectContext,
) -> Result<(), i32>
```

Behavior:

1. If `project_ctx.manifest` has no `[dependencies.go]`, return `Ok(())` immediately. No cache-dir stat, no work.
2. Acquire an exclusive lockfile at `~/.lisette/cache/typedefs/.lis-regen.lock` (use `fs2`, same crate as `add.rs:449-476`). This serializes concurrent `lis check` invocations racing on the same empty cache after upgrade. Release on drop.
3. Build a `GoWorkspace` pointing at `project_ctx.typedef_cache_dir`.
4. For each `(module_path, GoDependency)` in `manifest.dependencies.go`:
   a. Construct a `GoModule { path, version }`.
   b. Check whether any package for that module already has a typedef file under the current-version cache dir. If yes, skip (reconcile itself is a no-op on fully cached modules, but an early skip avoids calling `go get` and `go list -m` for the common post-upgrade case of "already regenerated in a previous command").
   c. On first miss across the whole iteration, emit the header: `print_progress("Regenerating Go typedefs for lis v{CARGO_PKG_VERSION}")`. Guarded by a `bool` so it fires at most once.
   d. Emit per-module line: `print_progress("Regenerating {module}@{version}")`.
   e. Call `workspace.reconcile(&module)`.
   f. On error: collect into a `Vec<(String, String)>` of `(module, error_message)`. Do NOT abort the iteration.
5. After iteration, if the failure vec is non-empty, print each as a warning line (use existing `print_warning` helper if present, otherwise `print_progress` with a `!` marker matching `lis add` conventions). Example:
   ```
     ! Failed to regenerate github.com/foo/bar@v1.2.3: <error>
   ```
6. Return `Ok(())` regardless of partial failure. Semantic analysis proceeds. For modules that failed, `missing_go_typedef` will fire downstream with the new hint, which the user can act on. For successes, imports resolve normally.

### 3. Call sites

Wire `ensure_typedefs_for_current_version` into three handlers, placed *after* `ProjectContext` is built and *after* `check_toolchain_version` runs (inside `TypedefLocator::from_project[_with_manifest]`), but *before* the typedef locator actually loads files. The cleanest spot is: call it as the first step after the `ProjectContext` is constructed, before any typedef resolution.

Call sites:

- `crates/cli/src/handlers/check.rs` — before `TypedefLocator::from_project()` at line 64.
- `crates/cli/src/handlers/build.rs` — before `TypedefLocator::from_project_with_manifest()` at line 25.
- `crates/cli/src/handlers/run.rs` — project-mode branch only. The standalone branch (`run_standalone`, line 53-102) must NOT call it; standalone mode has no manifest and forbids third-party Go imports anyway.

Handlers NOT touched:

- `add` — already reconciles as part of its main flow.
- `fmt` — does not load typedefs.
- `doc` — only reads the stdlib index (`crates/cli/src/handlers/doc.rs`).
- `lsp` — no code change. The widened diagnostic hint (see above) carries `Run \`lis check\`` on every missing Go import, which is what a user hovering red squiggles post-upgrade will see. Follow-up idea: a one-shot `window/showMessage` on LSP init when the current-version cache dir is absent and the manifest has Go deps — non-blocking, no shell-out, ~15 lines. Not in this PR.

### 4. Toolchain pin ordering

`check_toolchain_version` (`crates/deps/src/project_manifest.rs:119-133`) is called from `TypedefLocator::from_project[_with_manifest]` before any typedef I/O. Auto-regen runs *after* the toolchain check passes, so a project pinning `lis = "0.1.9"` on a v0.1.10 machine still gets the loud toolchain error — we never silently regenerate against a mismatched toolchain.

### 5. Standalone mode

Gated implicitly: `run.rs` only calls the helper in the project branch. `check` and `build` are project-only already. No explicit `standalone_mode` check needed in the helper itself.

### 6. Concurrency

File lock at `~/.lisette/cache/typedefs/.lis-regen.lock` (created alongside the cache dir if absent). Lock is process-wide for the duration of the regen loop. If another process already holds it, block until released (use `lock_exclusive`, not `try_lock_exclusive`) — the wait is bounded by the other process finishing its own regen, after which our iteration finds everything cached and exits in one pass of `exists()` checks.

### 7. Tests

Add to `tests/spec/graph/` (mirroring the style of `graph_declared_dep_missing_typedef` at `tests/spec/graph/mod.rs:206-231`):

1. **Post-upgrade regen happy path.** Populate a fake cache under `lis@v{OLD}/` for a declared dep, leave `lis@v{CURRENT}/` empty, set up a mock reconcile that writes the expected typedef to the current-version dir, run `check`, assert no `missing_go_typedef` diagnostic AND assert the current-version dir was written.
2. **Partial failure.** Two declared deps, reconcile for one succeeds and for the other fails. Assert: success dep has no diagnostic, failure dep gets `missing_go_typedef` with the new `lis add <module>@<version>` hint.
3. **Idempotency.** Current-version cache already fully populated. Assert: zero reconcile calls, no header line emitted, `check` runs clean.
4. **Standalone mode.** `lis run foo.lis` with a bare file. Assert: regen helper is not invoked (easiest via not calling from that code path).
5. **Toolchain mismatch.** Manifest pins an older lis version. Assert: toolchain error fires before regen helper runs.
6. **Lockfile contention.** Less critical; existing `add.rs` lock tests provide the pattern if we want to reuse infra.

May require a new test helper for "fake typedef cache under a temp home dir" — none exists today (research finding 10). Keep this minimal: a `tempfile::TempDir` plus a small builder that writes `lis@v{X}/{module}@{version}/{pkg}.d.lis` files with given content.

## Implementation order

1. Fix the diagnostic hint (`module_graph.rs`) and update the snapshot in `tests/spec/graph/mod.rs:230` and any related fixtures. This alone is shippable and meaningfully improves the issue for users who hit the error anyway.
2. Scaffold `crates/cli/src/go_regen.rs` with `ensure_typedefs_for_current_version`, lockfile, skip-if-no-go-deps short-circuit. Unit-test the short-circuit path.
3. Wire into `check`, `build`, project-mode `run`.
4. Add the graph tests (happy path, partial failure, idempotency, standalone, toolchain mismatch).
5. Manual reproduction against the #84 scenario: isolated HOME, `lis add` under v{X}, rename cache dir to simulate upgrade, `lis check` should regenerate silently; flipping the rename to a broken module should surface the warning line and the updated hint.

## File touch list

- `crates/diagnostics/src/module_graph.rs` — hint text.
- `crates/cli/src/go_regen.rs` — new file.
- `crates/cli/src/lib.rs` — module declaration.
- `crates/cli/src/handlers/check.rs` — one call.
- `crates/cli/src/handlers/build.rs` — one call.
- `crates/cli/src/handlers/run.rs` — one call in project branch.
- `tests/spec/graph/mod.rs` — new cases.
- Possibly new `tests/spec/graph/` helper for fake cache dir setup.
- `bugs.md` — mark bug #10 as `[FIXED]` once the hint lands.
