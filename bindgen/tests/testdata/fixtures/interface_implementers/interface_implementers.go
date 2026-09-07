package interface_implementers

type Node struct{ Value int }

type Hook interface {
	Run(n *Node)
}

type HookFunc func(*Node)

func (h HookFunc) Run(n *Node) { h(n) }

type Visitor interface {
	Visit(n *Node)
}

type VisitorFunc func(*Node)

func (f VisitorFunc) Visit(n *Node) { f(n) }

type Reader struct{}

func (Reader) Visit(n *Node) { _ = n.Value }

type Inspector interface {
	Inspect(n *Node)
}

type Printer struct{}

func (Printer) Inspect(n *Node) { _ = n.Value }

type Orphan interface {
	Touch(n *Node)
}

type NamedHook interface {
	Hook
	Name() string
}

type Marker interface {
	Mark(n *Node)
}

type scribe struct{}

func (scribe) Mark(n *Node) { n.Value++ }

type Store[T any] interface {
	Put(n *Node, value T)
}

type IntStore struct{}

func (IntStore) Put(n *Node, value int) { n.Value = value }

var _ Store[int] = IntStore{}

type Box[T any] struct{ Value T }

type Applier[T any] interface {
	Apply(b *Box[T])
}

type GenericApplier[T any] struct{}

func (*GenericApplier[T]) Apply(b *Box[T]) {
	var zero T
	b.Value = zero
}

type StringApplier interface {
	Apply(b *Box[string])
}

var _ StringApplier = (*GenericApplier[string])(nil)

type Labeled interface {
	Applier[string]
	Label() string
}

type LabeledApplier struct{}

func (LabeledApplier) Apply(b *Box[string]) { b.Value = "" }
func (LabeledApplier) Label() string        { return "" }

type Peeker[T any] interface {
	Peek(b *Box[T])
}

type IntPeeker struct{}

func (IntPeeker) Peek(b *Box[int]) { _ = b.Value }

type Pair[K comparable, V any] interface {
	Set(m map[K]V, k K, v V)
}

type StringIntPair struct{}

func (StringIntPair) Set(m map[string]int, k string, v int) { m[k] = v }

var _ Pair[string, int] = StringIntPair{}

type Router interface {
	Serve(n *Node)
}

type Mux struct{ served int }

func (m *Mux) Serve(n *Node) {
	m.served++
	n.Value = m.served
}

type Bundle interface {
	Run(n *Node)
	Name() string
}

type Wrapped struct {
	Hook
}

func (Wrapped) Name() string { return "" }
