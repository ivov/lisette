// Fixture: a Go package declaring types whose names collide with Lisette prelude
// generics. `Option[T]` (arity 1) collides with prelude `Option<T>`, so its
// references must be package-qualified while the synthesized `Option<Field>`
// wrapper stays bare. `Result` (arity 0) does not collide with `Result<T, E>`
// (arity 2), so it stays bare.
package prelude_collision

type Option[T comparable] struct {
	Key   string
	Value T
}

func (o Option[T]) Selected(selected bool) Option[T] { return o }

func NewOption[T comparable](key string, value T) Option[T] {
	return Option[T]{Key: key, Value: value}
}

type Holder struct {
	Chosen Option[int]
}

// WithWidth returns the interface itself; the nilable return becomes the prelude
// `Option<Field>` wrapper.
type Field interface {
	Update() error
	WithWidth(int) Field
}

type Input struct{}

func (i *Input) Update() error         { return nil }
func (i *Input) WithWidth(w int) Field { return i }

func TakeField(f Field) {}

type Result interface {
	Rows() int
}
