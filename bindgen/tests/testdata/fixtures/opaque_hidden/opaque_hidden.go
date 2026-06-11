package opaque_hidden

type secret struct{ x int }

func (secret) Whisper() int { return secret{}.x }

// Box has no exported fields, so it renders as an opaque `pub type`; it hides the
// unexported embed `secret`, so it must carry `#[go(hidden_embed)]`.
type Box struct {
	secret
}

func (Box) Open() int { return 0 }
