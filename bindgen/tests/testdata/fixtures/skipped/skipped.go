package skipped

import "unsafe"

// Should be skipped: unsafe.Pointer
func UnsafeFunc(p unsafe.Pointer) {}

// Should be skipped: unexported field type
type hasPrivate struct{ x int }
type Public struct {
	Field hasPrivate
}

// Should be skipped: constrained generic (non-any)
type Ordered interface {
	~int | ~float64
}

func SkippedMin[T Ordered](a, b T) T { return a }

// Skip in param - tests convertFunc param skip path
func BadParam(x struct{ Y int }) {}

// Skip in Result return - tests analyzeReturns Result skip path
func BadResultReturn() (struct{ Y int }, error) { return struct{ Y int }{}, nil }

// Skip in Option return - tests analyzeReturns Option skip path
func BadOptionReturn() (v struct{ Y int }, ok bool) { return struct{ Y int }{}, true }

// Should skip: function with unsafe.Pointer param
func UnsafeParam(p unsafe.Pointer) int { return 0 }

// Should skip: function returning unsafe.Pointer
func UnsafeReturn() unsafe.Pointer { return nil }

// Should skip: struct with unsafe field
type UnsafeStruct struct {
	Ptr unsafe.Pointer
	Val int
}

// Should NOT skip: normal struct
type SafeStruct struct {
	Name  string
	Value int
}

// Method on safe struct
func (s SafeStruct) GetName() string { return s.Name }

// More unsafe types

func BadUnsafeFunc(p unsafe.Pointer) {}

type HasUnsafe struct {
	Ptr unsafe.Pointer
}

// Nested anonymous structs - tests skip propagation in toLisetteInner
type NestedAnon struct {
	SliceAnon  []struct{ X int }       // Slice with skipped elem
	ArrayAnon  [4]struct{ X int }      // Array with skipped elem
	MapKeyAnon map[struct{ X int }]int // Map with skipped key
	MapValAnon map[int]struct{ X int } // Map with skipped value
	PtrAnon    *struct{ X int }        // Pointer to skipped type
	ChanAnon   chan struct{ X int }    // Chan with skipped elem
}

// Named function type referencing an unrepresentable type; must emit as an
// opaque `pub type` placeholder so downstream references still bind.
type OpaqueFunc func(h hasPrivate) hasPrivate

func MakeOpaqueFunc() OpaqueFunc  { return nil }
func UseOpaqueFunc(fn OpaqueFunc) {}

var DefaultOpaqueFunc OpaqueFunc

// Should skip: bare fixed-size array param has no inverse-conversion machinery
// in emit, so generated Go would receive []T where [N]T is required.
func TakesArray(addr [4]byte) {}

// Should skip: array param on the receiver method, same reason.
type ArrayParamHolder struct{}

func (a ArrayParamHolder) Set(addr [4]byte) {}

// Should skip: array as map key produces an uncomparable Lisette type.
type ArrayKeyHolder struct {
	Pairs map[[2]uint16]int
	Name  string
}

// Should skip: array used as map key in a parameter.
func TakesArrayKeyMap(m map[[2]uint16]int) {}

// Aliases collapse to bare arrays via *types.Alias RHS recursion. Both
// the param and the map-key positions through an alias must skip.
type AliasToArray = [4]byte

func TakesAliasArray(addr AliasToArray) {}

type AliasArrayKeyHolder struct {
	Pairs map[AliasToArray]int
	Name  string
}

// Arrays inside other structural positions: slice element, map value, and
// pointer must all propagate the skip rather than leaking a slice-shaped
// Lisette type.
type ArrayInStructPositions struct {
	SliceOfArr  [][3]int
	MapValArr   map[string][3]int
	PtrToArr    *[3]int
	DirectField [3]int
}
