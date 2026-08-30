// Exercises the superseded_by config override: curated symbols the Go team
// replaced, each emitting #[go(superseded_by, "<replacement>")] at its declaration.
package superseded_by

// Ints sorts a slice of ints. The superseded_by override names its replacement.
func Ints(x []int) {}

// Search has no configured replacement, so it carries no marker.
func Search(n int) int { return n }

// Sorter carries one superseded method and one ordinary one.
type Sorter struct{}

// Apply is superseded on the method path, keyed as "Sorter.Apply".
func (s *Sorter) Apply(x []int) {}

// Keep has no configured replacement.
func (s *Sorter) Keep(x []int) {}
