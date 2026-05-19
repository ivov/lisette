package interface_returns

import "io"

type Stringer interface {
	String() string
}

type Source interface {
	Get() Stringer
}

type Factory interface {
	NewStringer() Stringer
}

type Holder struct{ s Stringer }

func (h *Holder) Get() Stringer { return h.s }

type concreteString struct{}

func (concreteString) String() string { return "" }

func ProvenConcrete() Stringer { return concreteString{} }

type concretePtr struct{}

func (*concretePtr) String() string { return "" }

func ProvenPointer() Stringer { return &concretePtr{} }

type Producer interface {
	Make() (Stringer, error)
}

type ReaderSource interface {
	Open() io.Reader
}

func NewMaybe() Stringer { return nil }

func Delegate() Stringer { return NewMaybe() }

func DelegateViaVar() Stringer {
	var r Stringer = NewMaybe()
	return r
}

var cachedMaybe Stringer = NewMaybe()

func DelegateViaPkgVar() Stringer { return cachedMaybe }

var cachedReassigned Stringer = concreteString{}

func init() { cachedReassigned = nil }

func DelegateViaReassignedPkgVar() Stringer { return cachedReassigned }

var cachedClearable Stringer = concreteString{}

func Clear() { cachedClearable = nil }

func DelegateViaClearablePkgVar() Stringer { return cachedClearable }

func AnonInterfaceReturn() interface{ String() string } { return nil }

type StringerAlias = Stringer

func MaybeAlias() StringerAlias { return nil }

type Widget struct{ ID int }

type WidgetAlias = *Widget

func MaybeWidgetAlias() WidgetAlias { return nil }
