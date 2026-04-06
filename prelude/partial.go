package lisette

import "fmt"

type PartialTag int

const (
	PartialOk PartialTag = iota
	PartialErr
	PartialBoth
)

type Partial[T any, E any] struct {
	Tag    PartialTag
	OkVal  T
	ErrVal E
}

func MakePartialOk[T any, E any](arg T) Partial[T, E] {
	return Partial[T, E]{Tag: PartialOk, OkVal: arg}
}

func MakePartialErr[T any, E any](arg E) Partial[T, E] {
	return Partial[T, E]{Tag: PartialErr, ErrVal: arg}
}

func MakePartialBoth[T any, E any](val T, err E) Partial[T, E] {
	return Partial[T, E]{Tag: PartialBoth, OkVal: val, ErrVal: err}
}

func (p Partial[T, E]) IsOk() bool   { return p.Tag == PartialOk }
func (p Partial[T, E]) IsErr() bool  { return p.Tag == PartialErr }
func (p Partial[T, E]) IsBoth() bool { return p.Tag == PartialBoth }

func (p Partial[T, E]) Ok() Option[T] {
	if p.Tag == PartialOk || p.Tag == PartialBoth {
		return Option[T]{Tag: OptionSome, SomeVal: p.OkVal}
	}
	return Option[T]{Tag: OptionNone}
}

func (p Partial[T, E]) Err() Option[E] {
	if p.Tag == PartialErr || p.Tag == PartialBoth {
		return Option[E]{Tag: OptionSome, SomeVal: p.ErrVal}
	}
	return Option[E]{Tag: OptionNone}
}

func (p Partial[T, E]) UnwrapOr(def T) T {
	if p.Tag == PartialOk || p.Tag == PartialBoth {
		return p.OkVal
	}
	return def
}

func (p Partial[T, E]) UnwrapOrElse(f func(E) T) T {
	if p.Tag == PartialOk || p.Tag == PartialBoth {
		return p.OkVal
	}
	return f(p.ErrVal)
}

func (p Partial[T, E]) String() string {
	switch p.Tag {
	case PartialOk:
		return fmt.Sprintf("Ok(%v)", p.OkVal)
	case PartialErr:
		return fmt.Sprintf("Err(%v)", p.ErrVal)
	default:
		return fmt.Sprintf("Both(%v, %v)", p.OkVal, p.ErrVal)
	}
}

func PartialMap[T any, U any, E any](p Partial[T, E], f func(T) U) Partial[U, E] {
	switch p.Tag {
	case PartialOk:
		return Partial[U, E]{Tag: PartialOk, OkVal: f(p.OkVal)}
	case PartialBoth:
		return Partial[U, E]{Tag: PartialBoth, OkVal: f(p.OkVal), ErrVal: p.ErrVal}
	default:
		return Partial[U, E]{Tag: PartialErr, ErrVal: p.ErrVal}
	}
}

func PartialMapErr[T any, E any, F any](p Partial[T, E], f func(E) F) Partial[T, F] {
	switch p.Tag {
	case PartialErr:
		return Partial[T, F]{Tag: PartialErr, ErrVal: f(p.ErrVal)}
	case PartialBoth:
		return Partial[T, F]{Tag: PartialBoth, OkVal: p.OkVal, ErrVal: f(p.ErrVal)}
	default:
		return Partial[T, F]{Tag: PartialOk, OkVal: p.OkVal}
	}
}
