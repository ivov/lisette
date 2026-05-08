// Fixture: constants typed by a named integer type that lives in a foreign
// package must not be promoted into a fabricated local enum named after that
// foreign type. The local-typed `Priority` block is a positive control to
// confirm the foreign-type guard does not over-fire.
package foreign_typed_const

import "time"

const (
	NoExpiration      time.Duration = -1
	DefaultExpiration time.Duration = 0
)

type Priority int

const (
	PriorityLow    Priority = -1
	PriorityNormal Priority = 0
	PriorityHigh   Priority = 1
)
