package opaque_hidden

type secret struct{ x int }

func (secret) Whisper() int { return secret{}.x }

// Box has no exported fields, only the unexported embed `secret`; the embed is
// emitted faithfully as `embed secret` (an opaque `#[go(unexported)]` type) so
// Whisper promotes through it at the correct depth.
type Box struct {
	secret
}

func (Box) Open() int { return 0 }
