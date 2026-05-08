// Mirrors the urfave/cli/v2 SliceFlag pattern: a generic struct whose type
// constraint cannot be represented in Lisette. The placeholder must keep its
// arity so the dependent alias and the method's impl block stay in sync.
package skipped_generic

// Base interface to be embedded — its presence makes Target's NumEmbeddeds > 0,
// which is what causes recognizeBound to reject Target as a representable bound.
type Base interface {
	Apply()
}

// Constraint with both an embedded interface and a method referencing the
// type parameter. recognizeBound rejects this shape, so any TypeParam bound
// by it becomes unrepresentable.
type Target[E any] interface {
	Base
	Set([]E)
}

// SliceFlag's `T Target[E]` constraint is unrepresentable, so the type is
// skipped — but the opaque placeholder, the impl block below, and the alias
// below must all agree on arity 3.
type SliceFlag[T Target[E], S ~[]E, E any] struct {
	Inner T
	Value S
}

func (x *SliceFlag[T, S, E]) Apply() {}

// Concrete target satisfying Target[string].
type StringTarget struct{}

func (s *StringTarget) Apply()         {}
func (s *StringTarget) Set(_ []string) {}

// Dependent alias instantiating the skipped generic — this is the line that
// fails to compile when the placeholder loses its arity.
type StringSliceFlag = SliceFlag[*StringTarget, []string, string]
