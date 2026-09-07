package writable_capability

import "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/writable_capability/internal/inner"

// Pointer-to-scalar fields render as Option<T>, which carries no write permission.
type ScalarPointers struct {
	Text *string
	Flag *bool
	Num  *int
}

type ScalarWrapper struct {
	Value ScalarPointers
}

type Celsius float64

type NamedScalarPointers struct {
	Temp *Celsius
}

type NamedScalarWrapper struct{ Value NamedScalarPointers }

type ArrayOfScalarPointers struct {
	Nums [3]*int
}

type ArrayOfScalarWrapper struct{ Value ArrayOfScalarPointers }

// A pointer to a struct still renders as a writable Ref.
type Mixed struct {
	Text *string
	Node *Scalars
}

type Scalars struct {
	Text string
	Flag bool
}

type MixedWrapper struct {
	Value Mixed
	Data  []ScalarPointers
	Grid  [2]ScalarPointers
}

// Hidden fields carry nothing once another field is emitted.
type Hidden struct {
	Text *string
	buf  []byte
}

type HiddenWrapper struct{ Value Hidden }

type hiddenBuf struct{ buf []byte }

type HoldsHidden struct {
	Inner hiddenBuf
	Text  *string
}

type HoldsHiddenWrapper struct{ Value HoldsHidden }

type MixedPool struct {
	Conns []*inner.Conn
	Text  *string
}

type MixedPoolWrapper struct{ Value MixedPool }

// A struct with no emitted field becomes an opaque type, where Go's view decides.
type Opaque struct{ routes []string }

type OpaqueWrapper struct {
	Opaque
	Value Opaque
}

type Pool struct {
	Conns []*inner.Conn
}

type PoolWrapper struct{ Value Pool }

// An unexported embed renders opaque too, with or without the pointer.
type state struct{ n *int }

type EmbedsPointer struct{ *state }

type EmbedsValue struct{ state }

type EmbedsWrapper struct {
	A EmbedsPointer
	B EmbedsValue
}

// An unexported struct that implements an exported interface renders as that interface.
type Getter interface{ Get() string }

type hiddenGetter struct{ Text *string }

func (h hiddenGetter) Get() string { return "" }

type HoldsGetter struct{ H hiddenGetter }

type HoldsGetterWrapper struct{ Value HoldsGetter }

type hiddenError struct{ Buf []byte }

func (e hiddenError) Error() string { return "" }

type HoldsError struct{ E hiddenError }

type HoldsErrorWrapper struct{ Value HoldsError }

// A newtype renders its underlying type in non-nilable form.
type PtrArray [2]*Scalars

type ArrayWrapper struct{ Arr PtrArray }

func Mutate(s ScalarPointers, w ScalarWrapper, m Mixed) {
	*s.Text = "x"
	*w.Value.Num = 1
	m.Node.Text = "y"
}
