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
	SliceAnon  []struct{ X int }         // Slice with skipped elem
	ArrayAnon  [4]struct{ X int }        // Array with skipped elem
	MapKeyAnon map[struct{ X int }]int   // Map with skipped key
	MapValAnon map[int]struct{ X int }   // Map with skipped value
	PtrAnon    *struct{ X int }          // Pointer to skipped type
	ChanAnon   chan struct{ X int }      // Chan with skipped elem
}
