# Changelog

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
