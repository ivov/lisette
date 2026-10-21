// Fixture: a generic type's comparability can depend on its type arguments.
package generic_comparability

// Comparable only when T is: the hidden field holds a T.
type Box[T any] struct {
	Label string
	value T
}

// Comparable only when V is. K appears in no hidden field.
type Entry[K comparable, V any] struct {
	Key   K
	value V
}

// Never comparable: the hidden slice does not depend on T.
type Batch[T any] struct {
	Size  int
	items []T
}

// Comparable only when T is, through the visible field: the hidden one does not depend on T.
type Tagged[T any] struct {
	Value T
	id    int
}

// Comparable only when T is, with no visible field.
type Cell[T any] struct {
	value T
}

// Comparable only when E is: the hidden array holds Es.
type Grid[A ~[2]E, E any] struct {
	Name  string
	cells A
}

// Never comparable: the hidden field is a slice.
type Run[S ~[]E, E any] struct {
	Name  string
	items S
}

// Never comparable: the shape names itself, and it holds a slice.
type Loop[T ~struct {
	Next  T
	Items []int
}] struct {
	value T
}

// Never comparable: the hidden field is a map.
type Index[M ~map[K]V, K comparable, V any] struct {
	Name    string
	entries M
}
