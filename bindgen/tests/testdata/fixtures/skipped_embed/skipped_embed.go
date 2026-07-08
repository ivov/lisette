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

// Embeds Box[struct{...}] with a tagged anonymous struct type argument, which is
// unrepresentable, so bindgen skips the embed; Host.M must stay flattened, not
// dropped with it. Distinct from Widget above: the gate propagates through a
// generic type argument, not a direct internal-package embed.
type Host struct {
	Box[struct {
		K int `json:"k"`
	}]
	Y int
}
