package inner

import "time"

// Wrapped basic types — the Ginkgo decorator pattern.
type NodeTimeout time.Duration
type Offset uint
type SpecPriority int

// Wrapped slice — the Labels pattern.
type Labels []string

// Wrapped unexported type (focusType pattern). The const is publicly
// reachable via re-export but the type itself stays unnamed outside.
type focusType bool

const Focus = focusType(true)
const Pending = focusType(false)
