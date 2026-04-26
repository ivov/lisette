package structs

import "io"

// Basic struct with exported fields
type Point struct {
	X int
	Y int
}

// Mixed visibility (unexported fields ignored)
type Mixed struct {
	Public  string
	private int
}

// Empty struct
type Empty struct{}

// Struct with documented fields - tests emitType field doc path
type Documented struct {
	// The unique identifier
	ID int
	// The display name
	Name string
}

// Anonymous struct cases

func ReturnsAnon() struct{ X int } {
	return struct{ X int }{X: 1}
}

var AnonField struct {
	Nested struct{ Y string }
}

// Embedding

// Struct embedding another struct
type Base struct {
	ID   int
	Name string
}

// GetID returns the ID
func (b Base) GetID() int { return b.ID }

// SetName sets the name
func (b *Base) SetName(name string) { b.Name = name }

type Extended struct {
	Base
	Extra string
}

// Shadowing: Shadower declares its own GetID, so Base.GetID should not be promoted
type Shadower struct {
	Base
}

// GetID overrides the promoted method from Base
func (s Shadower) GetID() string { return "shadowed" }

// Interface embedding
type ReadWriteCloser interface {
	io.Reader
	io.Writer
	io.Closer
}

// Struct embedding interface
type Wrapper struct {
	io.Reader
	Label string
}

// Function-typed fields

type Handler struct {
	OnSuccess func(result string)
	OnError   func(err error)
	Transform func(input []byte) ([]byte, error)
	Lookup    func(key string) (value int, ok bool) // Option pattern
	GetCoords func() (x, y, z float64)              // Tuple pattern
}

// Named function-type alias used as a struct field — same nilability as
// anonymous func fields
type Validator func(input string) error

type Form struct {
	Validate Validator
	Name     string
}

// Function that takes a callback
func WithCallback(cb func(int) bool) {}

// Function that returns a function
func MakeAdder(x int) func(int) int { return nil }

// Collection-of-pointer fields
type Project struct {
	Name    string
	Modules []*Node
	Lookup  map[string]*Node
}

// Recursive types

type Node struct {
	Value int
	Next  *Node
}

type Tree struct {
	Left  *Tree
	Right *Tree
	Data  string
}
