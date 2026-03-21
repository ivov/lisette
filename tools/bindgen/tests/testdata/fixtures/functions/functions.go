package functions

import "unsafe"

// Function types

type (
	NoArgs      func()
	WithArgs    func(int, string) bool
	Variadic    func(string, ...int) error
	MultiReturn func() (int, error)
)

// Variadic function - tests convertFunc variadic path
func Printf(format string, args ...any) {}

// Function with unnamed params - tests convertFunc unnamed param path
func Callback(int, string) bool { return false }

// Simple function types

type Handler func(int) error
type SimpleCallback func()

// Function type with unsafe param in struct field - triggers signatureToLisette skip path
type BadHandler struct {
	Process func(p unsafe.Pointer) int
}

// Function type with unsafe return in struct field
type BadFactory struct {
	Create func() unsafe.Pointer
}

// Variadic function type
type Logger struct {
	Log func(format string, args ...interface{})
}

// Function returning multiple values including error
type Fetcher struct {
	Fetch func(url string) ([]byte, error)
}

// Function type returning skipped type with error - tests analyzeReturns skip in Result path
type BadResultFn struct {
	Run func() (struct{ X int }, error)
}

// Function type returning skipped type with ok - tests analyzeReturns skip in Option path
type BadOptionFn struct {
	Check func() (v struct{ X int }, ok bool)
}

// Function type returning multiple skipped types - tests collectReturnTypes skip in tuple path
type TupleSkip struct {
	GetPair func() (struct{ A int }, struct{ B int })
}

// Function type with just error return - tests single error path
type ErrorOnly struct {
	Try func() error
}

// Node for testing pointer returns in comma-ok
type Node struct {
	Value int
}

// Comma-ok with pointer inner type - produces tuple to avoid
// ambiguity with single-pointer returns that also produce Option<Ref<T>>
func FindNode(key string) (node *Node, ok bool) { return nil, false }
