// Exercises the skip config override: curated symbols that Lisette cannot
// support, each emitted as a placeholder with #[go(skipped, "<reason>")].
package skip_override

// Find is skipped by the override.
func Find(target any) bool { return false }

// FindAll is not listed, so it keeps its signature.
func FindAll() []int { return nil }

// Target is a skipped var.
var Target int

// Limit is a skipped constant.
const Limit = 3

// Box is listed, but the override does not apply to a type.
type Box struct{ Value int }

// Sorter keeps its Find method: the override names only the top-level Find.
type Sorter struct{}

// Find on Sorter is not skipped.
func (Sorter) Find() bool { return true }
