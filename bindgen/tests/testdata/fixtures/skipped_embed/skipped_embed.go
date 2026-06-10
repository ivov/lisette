package skipped_embed

import "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/skipped_embed/internal/hidden"

// Exported embed of an internal-package type: bindgen skips the field, so Widget
// looks flat but must stay unembeddable (marked) with hidden.Engine.Run kept.
type Widget struct {
	hidden.Engine
	X int
}

type Box[T any] struct {
	V T
}

func (Box[T]) M() int { return 0 }

// Embeds Box[[2]int]: the array type argument is unrepresentable, so bindgen
// skips the field; Host.M must stay flattened, not dropped with the skipped embed.
type Host struct {
	Box[[2]int]
	Y int
}
