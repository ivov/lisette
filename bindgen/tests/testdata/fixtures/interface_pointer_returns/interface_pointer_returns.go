// Pointer-returning interface methods must be bound as Option<Ref<T>>
// regardless of name, so callers see the nilability the Go contract permits.
package interface_pointer_returns

type Widget struct{ ID int }

type HasWidget interface {
	GetWidget() *Widget
}

// Constructor naming must NOT exempt on the interface side. The contract
// permits nil from any implementer, even if a particular impl happens not to
// return nil. Snapshot must show Option<Ref<Widget>>.
type Factory interface {
	NewWidget() *Widget
}

// Navigation naming on an interface method also wraps.
type Tree interface {
	Find(id int) *Widget
	Next() *Widget
}

// Multi-method interface to confirm the wrap applies per pointer return,
// not as a one-shot.
type Container interface {
	GetWidget() *Widget
	Parent() *Container
	Children() []*Widget
}

// Concrete impl. Its GetWidget body addresses a struct field, so the existing
// concrete-side wrap pass exempts via isProvenNonNilReturn. The interface
// gains Option from Part 1; the resolver covariance closes the gap at use time.
type WidgetBox struct{ w Widget }

func (b *WidgetBox) GetWidget() *Widget { return &b.w }

// Named pointer type. Both interface and concrete sides wrap to
// `Option<Handle>`; the ABI classifier recognizes named pointer types
// as nilable so this lowers to the correct `Handle` Go signature.
type Handle *Widget

type HandleHolder interface {
	GetHandle() Handle
}

// Concrete impl with a body that is not proven non-nil (returns a field).
// Snapshot must show the same `Option<Handle>` shape on both sides to
// avoid the bug 2 mismatch for named pointer types.
type HandleBox struct{ h Handle }

func (b *HandleBox) GetHandle() Handle { return b.h }
