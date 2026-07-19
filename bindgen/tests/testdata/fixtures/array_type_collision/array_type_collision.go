// Fixture: a generic `Array` collides with the built-in, so its references qualify
// while a genuine `[16]byte` stays the bare `Array<byte, 16>`.
package array_type_collision

type Array[T any] struct {
	Elements []T
	Valid    bool
}

func (a Array[T]) Len() int { return len(a.Elements) }

func NewArray[T any]() Array[T] { return Array[T]{} }

type Holder struct {
	Ints   Array[int]
	Digest [16]byte
}

func TakeArray[T any](a Array[T]) {}

func Digest() [16]byte { return [16]byte{} }
