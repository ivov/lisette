package arrays

func Sum() [32]byte { return [32]byte{} }

func Take(addr [4]byte) {}

func Grid() [2][3]int { return [2][3]int{} }

func Rows() [][4]int { return nil }

func Ptr() *[8]byte { return nil }

// Array as a map key (arrays are comparable when the element is).
func TakesArrayKey(m map[[2]uint16]int) {}

func WithErr() ([32]byte, error) { return [32]byte{}, nil }

func Pair() ([2]int, [3]int) { return [2]int{}, [3]int{} }

type Digest = [16]byte

func Compute() Digest                { return Digest{} }
func TakeDigest(d Digest)            {}
func TakeDigestKey(m map[Digest]int) {}

type Holder struct {
	Data  [3]int
	Pairs map[string][4]byte
}

func (h Holder) First() [3]int { return h.Data }
func (h Holder) Set(a [3]int)  {}

// A nilable element in a nilable position is Option-wrapped like a slice
// element: a pointer to a scalar drops Ref, a pointer to a struct keeps it.
type Cell struct {
	Ptrs  [2]*int
	Nodes [2]*Cell
}
