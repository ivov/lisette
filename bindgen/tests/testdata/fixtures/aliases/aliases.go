package aliases

import "errors"

var errFixture = errors.New("fixture")

// Type aliases (using =)

type AliasString = string
type AliasIntSlice = []int
type AliasStringMap = map[string]int

// Function using type aliases
func TakeAlias(s AliasString) AliasIntSlice {
	return nil
}

// Function returning alias
func GetMap() AliasStringMap {
	return AliasStringMap{}
}

// Type definitions (new distinct types)

type MyInt int
type MyString string
type ID uint64

// Type definition over slice - hits convertType default path
type IntList []int

// Type definition over map
type StringMap map[string]int

// Function type aliases

// Handler is a function that handles requests.
type Handler func(request string) (response string, err error)

// Callback is a simple callback function.
type Callback func()

// Processor processes data.
type Processor func(data []byte) []byte

// Alias-to-array peels to the array type.
type Digest = [32]byte

func ComputeDigest() Digest {
	return Digest{}
}

// Matrix exercises nested container hops in field and newtype positions.
type Matrix [][]byte

// MakeIntList exercises the writable qualifier on a named container return.
func MakeIntList(n int) IntList {
	return make(IntList, n)
}

// Grid holds nested containers to exercise per-hop field qualifiers.
type Grid struct {
	Rows    [][]byte
	Lookup  map[string][]int
	Pointer *IntList
	Raw     **byte
}

// RefList exercises the plain newtype shape with per-hop qualifiers.
type RefList []*IntList

// PairOfLists exercises per-result qualifiers on a plain multi-value return.
func PairOfLists() (IntList, IntList) {
	return make(IntList, 1), make(IntList, 1)
}

// Bag exercises writable capability through a by-value struct.
type Bag struct {
	Items []int
}

// FillBag exercises the mutated by-value struct parameter.
func FillBag(b Bag) {
	b.Items[0] = 1
}

// BlankPair exercises per-hop qualifiers on a mutated array parameter.
func BlankPair(p [2][]byte) {
	p[0][0] = 0
}

// TouchKeys exercises the writable qualifier on pointer map keys.
func TouchKeys(m map[*IntList]int) {
	for k := range m {
		m[k] = 2
	}
}

// GridBox exercises the pointer-embed qualifier.
type GridBox struct {
	*Grid
}

// ListBox exercises the non-struct embed rendering.
type ListBox struct {
	IntList
}

// GridPack exercises the value-struct embed qualifier.
type GridPack struct {
	Grid
}

// Callbacks exercises the writable pointer return inside a function type.
type Callbacks struct {
	Load func(name string) (*IntList, error)
}

// FillAlias exercises the writable qualifier through a type alias.
func FillAlias(xs AliasIntSlice) {
	xs[0] = 1
}

// FillIntList exercises the mutated-pointer-parameter qualifier.
func FillIntList(l *IntList) {
	*l = append(*l, 1)
}

// BlankTopLeft exercises per-hop qualifiers on a nested mutated parameter.
func BlankTopLeft(rows [][]byte) {
	rows[0][0] = 0
}

// AppendByte exercises a result that views a heuristically mutable parameter.
func AppendByte(dst []byte, b byte) []byte {
	return append(dst, b)
}

// Pick exercises curated view overrides on a blind-spot result.
func Pick(f func([]string) []string, rows []string) []string {
	return f(rows)
}

// Store exercises the curated writable return on a body-less interface method.
type Store interface {
	Items() IntList
}

// Counter exercises the curated receiver override.
type Counter struct {
	Total int
}

// Bump exercises a receiver write hidden behind an indirect call.
func (c *Counter) Bump(f func(*int)) {
	f(&c.Total)
}

// FillVia exercises curated permission on a reserved parameter name.
func FillVia(f func([]int), self []int) {
	f(self)
}

// PairOfRows exercises per-hop qualifiers on a writable array return.
func PairOfRows() [2][]byte {
	return [2][]byte{make([]byte, 1), make([]byte, 1)}
}

// OpenIntList exercises the qualifier inside a Result wrapper.
func OpenIntList(ok bool) (IntList, error) {
	if !ok {
		return nil, errFixture
	}
	return make(IntList, 1), nil
}
