package generic_alias

type Box[T any] struct {
	V T
}

func (Box[T]) M() int { return 0 }

type Alias[T any] = Box[T]

type Host struct {
	Alias[int]
}

type Reader[T any] interface {
	Read() T
}

type ReaderAlias[T any] = Reader[T]

func Use[T ReaderAlias[int]](v T) int { return 0 }
