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
