# Changelog

## [0.1.8](https://github.com/ivov/lisette/compare/lisette-v0.1.7...lisette-v0.1.8) - 2026-04-12

- feat: groundwork for lis add command ([#55](https://github.com/ivov/lisette/pull/55)) ([`e4a15e7`](https://github.com/ivov/lisette/commit/e4a15e7a4937ad498d21f67a20b0e86f1e717596))
- refactor: reorganize deps crate ([`09beac3`](https://github.com/ivov/lisette/commit/09beac374f09f4766d67598a203d41eabf8a70bd))
- refactor: simplify bindgen invocation ([`262cc20`](https://github.com/ivov/lisette/commit/262cc20c20cad53d61415b0538f4cf9be7a65dc2))
- fix: reject relative-path imports with clear diagnostic ([#58](https://github.com/ivov/lisette/pull/58)) ([`21389f0`](https://github.com/ivov/lisette/commit/21389f0264e60da9d7dcf8eb6d8398bd2c82c810))
- fix: register impl blocks after sibling-file type definitions ([#57](https://github.com/ivov/lisette/pull/57)) ([`85a0d5f`](https://github.com/ivov/lisette/commit/85a0d5fe72f1c226fe8a59eacb33c2d7a9667359))
- refactor: simplify typedef resolver ([#50](https://github.com/ivov/lisette/pull/50)) ([`07a7a45`](https://github.com/ivov/lisette/commit/07a7a453b2deeef6660a5e2f56f66801af3012bc))

## [0.1.7](https://github.com/ivov/lisette/compare/lisette-v0.1.6...lisette-v0.1.7) - 2026-04-11

### Chore

- include license file in published crates ([#48](https://github.com/ivov/lisette/pull/48))

### Feat

- publish bindgen as a Go module ([#47](https://github.com/ivov/lisette/pull/47))
- compiler awareness of third-party Go deps ([#44](https://github.com/ivov/lisette/pull/44))

### Fix

- resolve Forall gracefully and add registration to fuzz target
- validate type parameter bounds on type definitions ([#43](https://github.com/ivov/lisette/pull/43))

## [0.1.6](https://github.com/ivov/lisette/compare/lisette-v0.1.5...lisette-v0.1.6) - 2026-04-09

### Feat

- add `completions` CLI command ([#39](https://github.com/ivov/lisette/pull/39))

### Fix

- minor cli adjustments ([#40](https://github.com/ivov/lisette/pull/40))
- deduplicate diagnostics for const type annotations
- deduplicate diagnostics for function signature annotations
- resolve non-generic type aliases as qualifiers cross-module ([#37](https://github.com/ivov/lisette/pull/37))

## [0.1.5](https://github.com/ivov/lisette/compare/lisette-v0.1.4...lisette-v0.1.5) - 2026-04-08

### Fixed

- skip pattern analysis on import cycle ([#34](https://github.com/ivov/lisette/pull/34))
- interface subtype satisfaction through type variables ([#31](https://github.com/ivov/lisette/pull/31))

## [0.1.4](https://github.com/ivov/lisette/compare/lisette-v0.1.3...lisette-v0.1.4) - 2026-04-07

### Added

- *(editors)* add info for helix ([#21](https://github.com/ivov/lisette/pull/21))

### Fixed

- add typo suggestions for CLI subcommands ([#23](https://github.com/ivov/lisette/pull/23))
- support octal escape sequences ([#22](https://github.com/ivov/lisette/pull/22))
- ice when calling generic type as function ([#28](https://github.com/ivov/lisette/pull/28))
- skip auto-generated stringer on user string + goString
- swap string method for go string method ([#17](https://github.com/ivov/lisette/pull/17))

## [0.1.3](https://github.com/ivov/lisette/compare/lisette-v0.1.2...lisette-v0.1.3) - 2026-04-06

### Added

- add version override to bindgen stdlib command

### Fixed

- add Partial<T, E> for non-exclusive (T, error) returns ([#18](https://github.com/ivov/lisette/pull/18))
- guard against stack overflow from chained postfix operators
- make prelude variant name registration collision-safe
- decouple diagnostic coloring from environment ([#6](https://github.com/ivov/lisette/pull/6))
- detect typed nils in Go interface wrapping

### Other

- replace DiscardedTailFact boolean with enum
- match nested files in lefthook format check glob
- regenerate stdlib typedefs

## [0.1.2](https://github.com/ivov/lisette/compare/lisette-v0.1.1...lisette-v0.1.2) - 2026-03-31

### Added

- add quickstart link to CLI help and redirect page
- show nil diagnostic for null, Nil, and undefined

### Fixed

- improve doc help text colors, examples, and description
- fold Range sub-expressions in AstFolder
- prevent OOM by lowering max parser errors to 50
- prevent subtraction overflow in span calculation
- lower parser max depth to 64 to prevent stack overflow
- lower parser max depth to prevent stack overflow under asan
- remove unnecessary borrow in nil diagnostic format

### Other

- improve CLI help consistency and hide internal commands

## [0.1.1](https://github.com/ivov/lisette/compare/lisette-v0.1.0...lisette-v0.1.1) - 2026-03-21

### Fixed

- ensure complete go.sum before running go build
- resolve prelude path for crates.io packaging

## [0.1.0](https://github.com/ivov/lisette/releases/tag/lisette-v0.1.0) - 2026-03-21

### Added

- initial release v0.1.0
