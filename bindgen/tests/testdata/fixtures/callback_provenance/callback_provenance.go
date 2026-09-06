package callback_provenance

import "io"

type Node struct{ Value int }

func Forward(f func(*Node), n *Node) { f(n) }

func ForwardValue(f func(Node), n Node) { f(n) }

func ForwardToWriter(w io.Writer, p []byte) { _, _ = w.Write(p) }

func helper(f func(*Node), n *Node) { f(n) }

func ForwardHelper(f func(*Node), n *Node) { helper(f, n) }

func identity(f func(*Node)) func(*Node) { return f }

func ForwardReturned(f func(*Node), n *Node) { identity(f)(n) }

func ForwardVariadic(f func(...*Node), n *Node) { f(n) }

type Holder struct{ Node *Node }

func ForwardHolder(f func(Holder), n *Node) { f(Holder{Node: n}) }

type Box struct{ callback func(*Node) }

func NewBox(f func(*Node)) *Box { return &Box{callback: f} }

func (b *Box) Invoke(n *Node) { b.callback(n) }

var internalVisit = func(n *Node) {}

func ForwardInternal(n *Node) { internalVisit(n) }

func InternalCaptured(n *Node) {
	f := func(n *Node) { _ = n.Value }
	func() { f(n) }()
}

func InternalFactory(n *Node) func() {
	f := func(n *Node) { _ = n.Value }
	return func() { f(n) }
}

type Seq[V any] func(yield func(V) bool)

func Chunks(data []byte) Seq[[]byte] {
	return func(yield func([]byte) bool) { yield(data) }
}

func ChunksAssigned(data []byte) Seq[[]byte] {
	f := func(yield func([]byte) bool) { yield(data) }
	return f
}

func chunksHelper(data []byte) func(func([]byte) bool) {
	return func(yield func([]byte) bool) { yield(data) }
}

func ChunksHelper(data []byte) Seq[[]byte] { return chunksHelper(data) }

func ChunksNested(data []byte) Seq[[]byte] {
	return func(yield func([]byte) bool) { func() { yield(data) }() }
}

func ChunksReassigned(data []byte) Seq[[]byte] {
	return func(yield func([]byte) bool) {
		defer func() { _ = yield }()
		old := yield
		yield = func(b []byte) bool { return old(b) }
		yield(data)
	}
}

func Lines(data []byte) func(yield func([]byte) bool) {
	return func(yield func([]byte) bool) { yield(data) }
}

func Mixed(data []byte) (Seq[int], func(func([]byte) bool)) {
	return func(yield func(int) bool) { yield(1) }, func(yield func([]byte) bool) { yield(data) }
}

func Dual(data []byte) (Seq[[]byte], func(func([]byte) bool)) {
	f := func(yield func([]byte) bool) { yield(data) }
	return f, f
}

func Twice(data []byte) (Seq[[]byte], Seq[[]byte]) {
	f := func(yield func([]byte) bool) { yield(data) }
	return f, f
}

type symbol struct {
	name  []byte
	value int
}

func walk(data []byte, fn func(symbol) error) error {
	var s symbol
	s.name = data[:1]
	return fn(s)
}

func Count(data []byte) int {
	n := 0
	_ = walk(data, func(s symbol) error {
		n += s.value
		return nil
	})
	return n
}

var dialHook func(n *Node)

func ForwardToHook(n *Node) {
	if dialHook != nil {
		dialHook(n)
	}
}

func wrapped(cb func(*Node)) func(*Node) {
	return func(n *Node) { cb(n) }
}

func ForwardWrapped(cb func(*Node), n *Node) { wrapped(cb)(n) }

func ForwardPhi(cb func(*Node), n *Node, flag bool) {
	f := func(x *Node) { cb(x) }
	if flag {
		f = func(x *Node) {}
	}
	f(n)
}

func noop(n *Node) {}

func ForwardChosen(cb func(*Node), n *Node, flag bool) {
	f := identity(noop)
	if flag {
		f = identity(cb)
	}
	f(n)
}

func variadicHelper(cb func(...*Node), ns ...*Node) { cb(ns...) }

func ForwardVariadicHelper(cb func(...*Node), n *Node) { variadicHelper(cb, n) }

func ForwardNested(cb func([][]*Node), n *Node) { cb([][]*Node{{n}}) }

var hook func(*Node)

func store(dst *func(*Node), cb func(*Node)) { *dst = cb }

func SetIndirect(cb func(*Node)) { store(&hook, cb) }

func InvokeIndirect(n *Node) { hook(n) }

type Crate struct{ cb func(*Node) }

func MakeCrate(cb func(*Node)) *Crate {
	c := &Crate{}
	store(&c.cb, cb)
	return c
}

func (c *Crate) Invoke(n *Node) { c.cb(n) }

func invokeConsumer(f func(func([]byte) bool)) {
	f(func(b []byte) bool { b[0] = 9; return true })
}

func ThroughConsumer(data []byte) {
	f := func(yield func([]byte) bool) { yield(data) }
	invokeConsumer(f)
}

func invokeReader(f func(func([]byte) bool)) {
	f(func(b []byte) bool { return len(b) > 0 })
}

func ThroughReader(data []byte) {
	f := func(yield func([]byte) bool) { yield(data) }
	invokeReader(f)
}

func sameNode(n *Node) *Node { return n }

func ForwardAlias(cb func(*Node), n *Node) { cb(sameNode(n)) }

func ForwardElement(cb func(*Node), nodes []*Node) { cb(nodes[0]) }

type Source interface{ Next() *Node }

func ForwardFromSource(cb func(*Node), src Source, n *Node) { cb(src.Next()) }

func outerIdentity(cb func(*Node)) func(*Node) { return identity(cb) }

func ForwardChosenNested(cb func(*Node), n *Node, flag bool) {
	f := outerIdentity(noop)
	if flag {
		f = outerIdentity(cb)
	}
	f(n)
}

func invoke(cb func(*Node), n *Node) { cb(n) }

func ForwardChosenWrapped(cb func(*Node), n *Node, flag bool) {
	f := wrapped(noop)
	if flag {
		f = wrapped(cb)
	}
	invoke(f, n)
}

func relay(cb func(*Node), n *Node) { cb(n) }

func invokeRelay(f func(func(*Node), *Node), cb func(*Node), n *Node) { f(cb, n) }

func ForwardKnown(cb func(*Node), n *Node) { invokeRelay(relay, cb, n) }

func ForwardKnownPhi(cb func(*Node), n *Node, flag bool) {
	f := relay
	if flag {
		f = func(cb func(*Node), n *Node) {}
	}
	f(cb, n)
}

func InternalKnown(n *Node) { invokeRelay(relay, noop, n) }

var hookWithDefault = noop

func SetDefaulted(cb func(*Node)) { store(&hookWithDefault, cb) }

func InvokeDefaulted(n *Node) { hookWithDefault(n) }

type Bin struct{ cb func(*Node) }

func NewBin() *Bin { return &Bin{cb: noop} }

func (b *Bin) Set(cb func(*Node)) { store(&b.cb, cb) }

func (b *Bin) Invoke(n *Node) { b.cb(n) }

var hooks [1]func(*Node)

func SetArray(cb func(*Node)) { hooks[0] = cb }

func InvokeArray(n *Node) { hooks[0](n) }

type Registry struct{ slots [1]func(*Node) }

func NewRegistry(cb func(*Node)) *Registry {
	r := &Registry{}
	r.slots[0] = cb
	return r
}

func (r *Registry) Invoke(n *Node) { r.slots[0](n) }
