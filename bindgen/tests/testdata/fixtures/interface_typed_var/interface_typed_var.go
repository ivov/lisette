package interfacetypedvar

import "io"

// EntityType mirrors world.EntityType from the issue: an exported interface
// implemented by unexported singleton marker types.
type EntityType interface {
	EncodeEntity() string
	BBox() int
}

// snowballType is an unexported empty-struct singleton implementing EntityType.
// Without interface inference its var leaks as Lisette's unit type `()`.
type snowballType struct{}

func (snowballType) EncodeEntity() string { return "minecraft:snowball" }
func (snowballType) BBox() int            { return 1 }

// SnowballType is the exported handle used in `==` comparisons.
var SnowballType snowballType

// arrowType is a second marker for the same interface.
type arrowType struct{}

func (arrowType) EncodeEntity() string { return "minecraft:arrow" }
func (arrowType) BBox() int            { return 2 }

var ArrowType arrowType

// nativeOrder mirrors encoding/binary.nativeEndian: an unexported singleton
// whose struct embeds another, so it has fields and previously got skipped
// entirely rather than flattened.
type nativeOrder struct {
	snowballType
}

func (nativeOrder) EncodeEntity() string { return "minecraft:native" }
func (nativeOrder) BBox() int            { return 3 }

var NativeType nativeOrder

// writerSingleton implements the cross-package io.Writer interface.
type writerSingleton struct{}

func (writerSingleton) Write(p []byte) (int, error) { return len(p), nil }

var DefaultWriter writerSingleton

// Discard forces the io import so io.Writer is a reachable candidate interface.
func Discard(w io.Writer) {}

// widget implements both a small and a large interface; inference should pick
// the most specific (largest method set).
type Renderer interface {
	Render() string
}

type Drawable interface {
	Render() string
	Bounds() int
}

type widget struct{}

func (widget) Render() string { return "w" }
func (widget) Bounds() int    { return 4 }

var DefaultWidget widget

// AnonParamIface is exported but not representable in Lisette: its method takes
// a tagged anonymous struct, which gates. A singleton implementing only it must
// fall back to the default `()` handling rather than be typed by an interface
// bindgen skips.
type AnonParamIface interface {
	Configure(data struct {
		X int `json:"x"`
	})
}

type anonParamMarker struct{}

func (anonParamMarker) Configure(data struct {
	X int `json:"x"`
}) {
}

var AnonParamSingleton anonParamMarker

// Cyclic exercises the self-referential representability probe: an exported
// interface whose method returns an unexported concrete type that implements the
// interface. Inferring the var's interface must not recurse forever.
type Cyclic interface {
	Self() cyclicMarker
}

type cyclicMarker struct{}

func (cyclicMarker) Self() cyclicMarker { return cyclicMarker{} }

var CyclicSingleton cyclicMarker
