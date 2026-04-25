package lisette

import (
	"encoding/json"
	"fmt"
)

type OptionTag int

const (
	OptionSome OptionTag = iota
	OptionNone
)

type Option[T any] struct {
	Tag     OptionTag
	SomeVal T
}

func MakeOptionSome[T any](arg T) Option[T] {
	return Option[T]{Tag: OptionSome, SomeVal: arg}
}

func MakeOptionNone[T any]() Option[T] {
	return Option[T]{Tag: OptionNone}
}

// OptionFromCommaOk wraps a Go comma-ok pair `(value, ok)` into an
// `Option[T]`.
func OptionFromCommaOk[T any](val T, ok bool) Option[T] {
	if ok {
		return Option[T]{Tag: OptionSome, SomeVal: val}
	}
	return Option[T]{Tag: OptionNone}
}

// OptionFromNilable wraps a Go nilable `T` (pointer, function, interface,
// map, slice, channel) into an `Option[T]`.
func OptionFromNilable[T any](val T, isNil bool) Option[T] {
	if isNil {
		return Option[T]{Tag: OptionNone}
	}
	return Option[T]{Tag: OptionSome, SomeVal: val}
}

func (opt Option[T]) IsSome() bool {
	return opt.Tag == OptionSome
}

func (opt Option[T]) IsNone() bool {
	return opt.Tag == OptionNone
}

func (opt Option[T]) UnwrapOr(def T) T {
	if opt.Tag == OptionSome {
		return opt.SomeVal
	}
	return def
}

func (opt Option[T]) UnwrapOrElse(f func() T) T {
	if opt.Tag == OptionSome {
		return opt.SomeVal
	}
	return f()
}

func (opt Option[T]) Filter(pred func(T) bool) Option[T] {
	if opt.Tag == OptionSome && pred(opt.SomeVal) {
		return opt
	}
	return Option[T]{Tag: OptionNone}
}

func (opt *Option[T]) Take() Option[T] {
	result := *opt
	*opt = Option[T]{Tag: OptionNone}
	return result
}

func (opt Option[T]) OrElse(f func() Option[T]) Option[T] {
	if opt.Tag == OptionSome {
		return opt
	}
	return f()
}

func (opt Option[T]) String() string {
	if opt.Tag == OptionSome {
		return fmt.Sprintf("Some(%v)", opt.SomeVal)
	}
	return "None"
}

func (opt Option[T]) IsZero() bool {
	return opt.Tag == OptionNone
}

func (opt Option[T]) MarshalJSON() ([]byte, error) {
	if opt.Tag == OptionNone {
		return []byte("null"), nil
	}
	return json.Marshal(opt.SomeVal)
}

func (opt *Option[T]) UnmarshalJSON(data []byte) error {
	if string(data) == "null" {
		opt.Tag = OptionNone
		return nil
	}
	opt.Tag = OptionSome
	return json.Unmarshal(data, &opt.SomeVal)
}

func OptionMap[T any, U any](opt Option[T], f func(T) U) Option[U] {
	if opt.Tag == OptionSome {
		return Option[U]{Tag: OptionSome, SomeVal: f(opt.SomeVal)}
	}
	return Option[U]{Tag: OptionNone}
}

func OptionAndThen[T any, U any](opt Option[T], f func(T) Option[U]) Option[U] {
	if opt.Tag == OptionSome {
		return f(opt.SomeVal)
	}
	return Option[U]{Tag: OptionNone}
}

func OptionOkOr[T any, E any](opt Option[T], err E) Result[T, E] {
	if opt.Tag == OptionSome {
		return Result[T, E]{Tag: ResultOk, OkVal: opt.SomeVal}
	}
	return Result[T, E]{Tag: ResultErr, ErrVal: err}
}

func OptionOkOrElse[T any, E any](opt Option[T], f func() E) Result[T, E] {
	if opt.Tag == OptionSome {
		return Result[T, E]{Tag: ResultOk, OkVal: opt.SomeVal}
	}
	return Result[T, E]{Tag: ResultErr, ErrVal: f()}
}

func OptionMapOr[T any, U any](opt Option[T], def U, f func(T) U) U {
	if opt.Tag == OptionSome {
		return f(opt.SomeVal)
	}
	return def
}

func OptionMapOrElse[T any, U any](opt Option[T], def func() U, f func(T) U) U {
	if opt.Tag == OptionSome {
		return f(opt.SomeVal)
	}
	return def()
}

func OptionZip[T any, U any](opt Option[T], other Option[U]) Option[Tuple2[T, U]] {
	if opt.Tag == OptionSome && other.Tag == OptionSome {
		return Option[Tuple2[T, U]]{Tag: OptionSome, SomeVal: Tuple2[T, U]{First: opt.SomeVal, Second: other.SomeVal}}
	}
	return Option[Tuple2[T, U]]{Tag: OptionNone}
}

func OptionFlatten[U any](opt Option[Option[U]]) Option[U] {
	if opt.Tag == OptionSome {
		return opt.SomeVal
	}
	return Option[U]{Tag: OptionNone}
}
