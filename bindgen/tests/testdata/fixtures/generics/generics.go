package generics

import "cmp"

// Basic generic types

type Box[T any] struct {
	Value T
}

type Pair[K, V any] struct {
	Key   K
	Value V
}

func Identity[T any](x T) T { return x }

// Generic interface - emits as opaque type with type params
type Transformer[T any] interface {
	Transform(T) T
}

// Generic container with methods

type Container[T any] struct {
	items []T
}

func (c *Container[T]) Add(item T) {
	c.items = append(c.items, item)
}

func (c *Container[T]) Get(index int) T {
	return c.items[index]
}

func (c *Container[T]) Len() int {
	return len(c.items)
}

func (c Container[T]) IsEmpty() bool {
	return len(c.items) == 0
}

// Complex generics with constraints

// Comparable constraint
func Max[T comparable](a, b T) T {
	return a
}

// Multiple type parameters with different constraints
type ConstrainedPair[K comparable, V any] struct {
	Key   K
	Value V
}

// Method on constrained generic
func (p ConstrainedPair[K, V]) GetKey() K {
	return p.Key
}

// Ordered constraint (type union)
type Ordered interface {
	~int | ~int8 | ~int16 | ~int32 | ~int64 |
		~uint | ~uint8 | ~uint16 | ~uint32 | ~uint64 |
		~float32 | ~float64 | ~string
}

// Function with ordered constraint
func Min[T Ordered](a, b T) T {
	if a < b {
		return a
	}
	return b
}

// Empty constraint (any)
func Clone[T any](v T) T {
	return v
}

// Multiple constraints
func Swap[T, U any](a T, b U) (U, T) {
	return b, a
}

// Inline union constraint with non-comparable types
func Either[T []int | []string](v T) T {
	return v
}

// cmp.Ordered constraint (named-identity recognizer)
func MinOrdered[T cmp.Ordered](a, b T) T {
	if a < b {
		return a
	}
	return b
}

// Slice-shape rewrite: S ~[]E with E: cmp.Ordered
func SortInts[S ~[]E, E cmp.Ordered](x S) S {
	return x
}

// Map-shape rewrite: single map, V any.
func MapClone[M ~map[K]V, K comparable, V any](m M) M {
	return m
}

// Map-shape rewrite: two maps, shared V (V any).
func MapCopy[M1, M2 ~map[K]V, K comparable, V any](dst M1, src M2) {
}

// Map-shape rewrite: two maps, shared V (V comparable).
func MapEqual[M1, M2 ~map[K]V, K, V comparable](m1 M1, m2 M2) bool {
	return false
}

// Map-shape rewrite: two maps, distinct V.
func MapEqualFunc[M1 ~map[K]V1, M2 ~map[K]V2, K comparable, V1, V2 any](m1 M1, m2 M2, eq func(V1, V2) bool) bool {
	return false
}
