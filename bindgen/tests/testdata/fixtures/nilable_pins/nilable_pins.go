// Fixture: pins for values that Go can set to nil where the analysis has no
// proof.
package nilable_pins

type Item struct{ N int }

type Job struct {
	// A pinned error field is Option<error>.
	Err error
	// An error field without a pin stays error.
	Cause error
}

var envs = map[string]map[string]string{}

// A pinned map result is Option.
func Env(key string) map[string]string { return envs[key] }

// A pinned named function type makes its pinned parameters Option.
type Visit func(path string, item *Item, err error) error

type Hooks struct {
	// A pinned function field makes its parameter Option, by position.
	Done func(string, error)
	// The pin does not reach the function type in a parameter.
	Walk func(*Item, func(*Item))
	// A pinned function field makes the value of its (T, error) result Option.
	Resolve func(string) (*Item, error)
	// The pin does not reach the function type in a result.
	Factory func() func() (*Item, error)
}

// A pin on a parameter makes the parameters of its function type Option.
func Each(items []*Item, f func(*Item) bool) {}

// The pin does not reach the function type in a result.
func Nested(f func(*Item) func(int) int) int { return 0 }

// A pinned (T, error) result makes only T Option.
func Find(key string) (*Item, error) { return nil, nil }

// A pinned function result makes the value of its (T, error) result Option.
func Fixed(item *Item) func(string) (*Item, error) {
	return func(string) (*Item, error) { return item, nil }
}

type Node interface{ Pos() int }

// A pinned interface method parameter is Option.
type Walker interface {
	Visit(node Node) Walker
}

// A method that implements a pinned interface method takes the same pin.
type Printer struct{}

func (p Printer) Visit(n Node) Walker {
	_ = n.Pos()
	return p
}

// A pinned interface method next to a second method.
type Sink interface {
	Take(item *Item)
	Flush()
}

type Lookalike struct{}

// A method with the shape of a pinned interface method takes no pin when its type does not implement the interface.
func (Lookalike) Take(item *Item) { _ = item.N }

type Part struct{}

// A method that only an embedding type supplies to a pinned interface takes no pin.
func (Part) Take(item *Item) { _ = item.N }

type Whole struct{ Part }

func (Whole) Flush() {}

// A pinned method of a generic interface.
type Consumer[T any] interface{ Use(x *T) }

type Box[T any] struct{ V T }

// A method of a generic type takes no pin.
func (Box[T]) Use(x *T) { _ = *x }
