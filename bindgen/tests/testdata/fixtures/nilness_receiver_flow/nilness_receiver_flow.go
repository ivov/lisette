// Fixture: receiver-return proofs must respect reassignment, and
// parameter-delegation summaries must carry receiver-return chains.
package nilness_receiver_flow

type Node struct {
	next, prev *Node
	Value      int
}

func (n *Node) Move(steps int) *Node {
	if n.next == nil {
		return n
	}
	for ; steps > 0; steps-- {
		n = n.prev
	}
	return n
}

func (n *Node) Reset() *Node {
	n.Value = 0
	return n
}

type List struct{ head *Node }

func (l *List) Init() *List {
	l.head = nil
	return l
}

func NewList() *List {
	return new(List).Init()
}

func FreshList() *List {
	return new(List).Init()
}

func (n *Node) Identity() *Node { return n }

func FromBoundMethod() *Node {
	var n *Node
	f := n.Identity
	return f()
}
