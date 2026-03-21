package lisette

import (
	"fmt"
	"runtime/debug"
)

type PanicValue struct {
	Value any
	Stack []byte
}

func (p PanicValue) String() string {
	return p.Message()
}

func (p PanicValue) Message() string {
	switch v := p.Value.(type) {
	case string:
		return v
	case error:
		return v.Error()
	default:
		return fmt.Sprint(v)
	}
}

func (p PanicValue) AsError() Option[error] {
	if err, ok := p.Value.(error); ok {
		return Option[error]{Tag: OptionSome, SomeVal: err}
	}
	return Option[error]{Tag: OptionNone}
}

func (p PanicValue) StackTrace() string {
	return string(p.Stack)
}

func RecoverBlock[T any](f func() T) Result[T, PanicValue] {
	var result T
	var pv PanicValue

	func() {
		defer func() {
			if r := recover(); r != nil {
				pv = PanicValue{Value: r, Stack: debug.Stack()}
			}
		}()
		result = f()
	}()

	if pv.Value != nil {
		return Result[T, PanicValue]{Tag: ResultErr, ErrVal: pv}
	}
	return Result[T, PanicValue]{Tag: ResultOk, OkVal: result}
}
