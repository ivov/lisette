package anon_structs

import "io"

// Return positions

// PlainReturn returns an anonymous struct directly.
func PlainReturn() struct{ X int } { return struct{ X int }{} }

// ResultReturn returns an anonymous struct alongside an error.
func ResultReturn() (struct{ X int }, error) { return struct{ X int }{}, nil }

// OptionReturn returns an anonymous struct with a comma-ok bool.
func OptionReturn() (v struct{ Label string }, ok bool) { return struct{ Label string }{}, true }

// TupleReturn returns two distinct anonymous structs.
func TupleReturn() (struct{ A int }, struct{ B int }) {
	return struct{ A int }{}, struct{ B int }{}
}

// Parameter position; constructible from Lisette because every field is exported.
func TakesAnon(p struct{ X int }) {}

// Collection, pointer, and channel element positions. The array element still
// gates, because fixed-size arrays are unrepresentable, so only that field is
// dropped.
type Carriers struct {
	Slice  []struct{ X int }
	MapKey map[struct{ X int }]int
	MapVal map[string]struct{ X int }
	Ptr    *struct{ X int }
	Chan   chan struct{ X int }
	Array  [4]struct{ X int }
}

// Struct fields, including a nested anonymous struct and one carrying an
// external (io) type.
type Holder struct {
	Direct struct{ X int }
	Nested struct{ Inner struct{ Deep int } }
	Writer struct{ W io.Writer }
}

// OnlyInArray's anonymous element appears nowhere else. The array gates, so the
// field drops; the element synthesis must not leak a type into the output.
type OnlyInArray struct {
	Items [3]struct{ Q int }
	Name  string
}

// Variable position.
var Anon struct {
	Hits   int64
	Misses int64
}
