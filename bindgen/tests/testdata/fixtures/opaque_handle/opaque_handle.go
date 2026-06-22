package opaque_handle

// Exposed as an opaque handle: direct producer vars exist below.
type chest struct {
	size        int
	titlePrefix string
}

var SmallChest = chest{size: 27, titlePrefix: "small"}
var LargeChest = chest{size: 54, titlePrefix: "large"}

type Submittable interface {
	Submit() error
}

type Menu struct {
	Title string
}

// Direct consumer: eligible.
func NewMenu(submittable Submittable, name string, c chest) Menu {
	return Menu{Title: name}
}

// Skips: a callback param would spell chest.
func TakesCallback(cb func(chest)) {}

// Skips: a variadic is a slice.
func UseAll(cs ...chest) {}

// Skips: an interface method would spell chest.
type Picker interface {
	Use(c chest)
}

// No direct producer (only nested []lump): no handle, consumer skips too.
type lump struct {
	weight int
}

var Lumps = []lump{{weight: 1}}

func UseLump(l lump) {}
