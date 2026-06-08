// Fixture: a package that declares its own `Option[T]` (colliding with the
// prelude) AND imports a different package sharing its own name. The import must
// get an alias so the self-qualified `prelude_alias_collision.Option` references
// stay unambiguous.
package prelude_alias_collision

import dup "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/prelude_alias_collision/widgets"

type Option[T comparable] struct {
	Value T
}

func NewOption[T comparable](value T) Option[T] {
	return Option[T]{Value: value}
}

func MakeWidget() dup.Widget {
	return dup.Widget{}
}
