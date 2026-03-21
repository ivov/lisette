package lisette

import "fmt"

type ResultTag int

const (
	ResultOk ResultTag = iota
	ResultErr
)

type Result[T any, E any] struct {
	Tag    ResultTag
	OkVal  T
	ErrVal E
}

func MakeResultOk[T any, E any](arg T) Result[T, E] {
	return Result[T, E]{Tag: ResultOk, OkVal: arg}
}

func MakeResultErr[T any, E any](arg E) Result[T, E] {
	return Result[T, E]{Tag: ResultErr, ErrVal: arg}
}

func (res Result[T, E]) IsOk() bool {
	return res.Tag == ResultOk
}

func (res Result[T, E]) IsErr() bool {
	return res.Tag == ResultErr
}

func (res Result[T, E]) Ok() Option[T] {
	if res.Tag == ResultOk {
		return Option[T]{Tag: OptionSome, SomeVal: res.OkVal}
	}
	return Option[T]{Tag: OptionNone}
}

func (res Result[T, E]) Err() Option[E] {
	if res.Tag == ResultErr {
		return Option[E]{Tag: OptionSome, SomeVal: res.ErrVal}
	}
	return Option[E]{Tag: OptionNone}
}

func (res Result[T, E]) UnwrapOr(def T) T {
	if res.Tag == ResultOk {
		return res.OkVal
	}
	return def
}

func (res Result[T, E]) UnwrapOrElse(f func(E) T) T {
	if res.Tag == ResultOk {
		return res.OkVal
	}
	return f(res.ErrVal)
}

func (res Result[T, E]) String() string {
	if res.Tag == ResultOk {
		return fmt.Sprintf("Ok(%v)", res.OkVal)
	}
	return fmt.Sprintf("Err(%v)", res.ErrVal)
}

func ResultMap[T any, U any, E any](res Result[T, E], f func(T) U) Result[U, E] {
	if res.Tag == ResultOk {
		return Result[U, E]{Tag: ResultOk, OkVal: f(res.OkVal)}
	}
	return Result[U, E]{Tag: ResultErr, ErrVal: res.ErrVal}
}

func ResultMapErr[T any, E any, F any](res Result[T, E], f func(E) F) Result[T, F] {
	if res.Tag == ResultErr {
		return Result[T, F]{Tag: ResultErr, ErrVal: f(res.ErrVal)}
	}
	return Result[T, F]{Tag: ResultOk, OkVal: res.OkVal}
}

func ResultMapOr[T any, U any, E any](res Result[T, E], def U, f func(T) U) U {
	if res.Tag == ResultOk {
		return f(res.OkVal)
	}
	return def
}

func ResultAndThen[T any, U any, E any](res Result[T, E], f func(T) Result[U, E]) Result[U, E] {
	if res.Tag == ResultOk {
		return f(res.OkVal)
	}
	return Result[U, E]{Tag: ResultErr, ErrVal: res.ErrVal}
}

func ResultOrElse[T any, E any, F any](res Result[T, E], f func(E) Result[T, F]) Result[T, F] {
	if res.Tag == ResultErr {
		return f(res.ErrVal)
	}
	return Result[T, F]{Tag: ResultOk, OkVal: res.OkVal}
}
