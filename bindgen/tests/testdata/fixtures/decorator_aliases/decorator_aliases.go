package decorator_aliases

import "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/decorator_aliases/internal/inner"

// Type aliases pointing into an internal package — should expose the
// alias's immediate wrapped type as a Lisette newtype so users can still
// construct values via `NodeTimeout(d)`.
type NodeTimeout = inner.NodeTimeout
type Offset = inner.Offset
type SpecPriority = inner.SpecPriority
type Labels = inner.Labels

// Re-exported consts whose declared type lives entirely in an internal
// package — should fall back to `Unknown` so the symbol stays usable
// as an opaque value passed into VarArgs<Unknown> parameters.
const Focus = inner.Focus
const Pending = inner.Pending
