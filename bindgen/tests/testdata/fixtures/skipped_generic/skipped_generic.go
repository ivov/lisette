// Mirrors the urfave/cli/v2 SliceFlag pattern: a generic type whose shape-collapsed
// type parameter (`S ~[]E`) cannot be represented on a type. The placeholder must
// keep its arity so the dependent alias and the method's impl block stay in sync.
package skipped_generic

// Embedded into Target, whose method set therefore flattens to include Apply.
type Base interface {
	Apply()
}

// A method-set interface (embed plus a method) is a representable bound, so
// binding it as `T: Target<E>` works.
type Target[E any] interface {
	Base
	Set([]E)
}

// The `S ~[]E` shape collapse is only supported for functions, so SliceFlag is
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
