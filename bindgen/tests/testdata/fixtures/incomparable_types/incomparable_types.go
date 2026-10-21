// Fixture: a type that Go cannot compare for a reason its rendering hides is marked incomparable.
package incomparable_types

// Marked: the slice that makes it incomparable is unexported.
type Buffer struct {
	Size int
	data []byte
}

// Marked: no field is visible.
type Handle struct {
	entries map[string]int
}

// Not marked: the visible slice already shows it is incomparable.
type Batch struct {
	Items []int
}

type hidden struct {
	xs []int
}

// Incomparable through the embed: the embedded opaque type carries the marker.
type Public struct {
	hidden
}

// Not marked: the hidden field is comparable.
type Point struct {
	X  int
	id int
}
