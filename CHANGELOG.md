# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
