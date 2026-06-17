// Fixtures for constructors that delegate to a nil-guarded helper, the shape
// cgo wrappers generate.
package constructor_delegation

type Widget struct{ id int }

type handle struct{ p int }

func makeHandle() *handle { return &handle{} }

func newWidget(h *handle) *Widget {
	if h == nil {
		return nil
	}
	return &Widget{}
}

func NewWidget() *Widget { return newWidget(makeHandle()) }

func NewNilWidget() *Widget { return nil }

func LookupWidget() *Widget { return newWidget(makeHandle()) }
