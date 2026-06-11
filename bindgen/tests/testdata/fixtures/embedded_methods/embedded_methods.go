// Regression guard for promoted-method receiver notation. Mirrors the
// Pool { *poolCommon } pattern in github.com/panjf2000/ants.
package embedded_methods

type Inner struct {
	Value int
}

func (i Inner) Read() int     { return i.Value }
func (i *Inner) Mutate(n int) { i.Value = n }

// Pointer embed.
type Host1 struct {
	*Inner
}

// Value embed.
type Host2 struct {
	Inner
}

// Pointer embed plus a directly-declared *Host3 method.
type Host3 struct {
	*Inner
}

func (h *Host3) Direct() int { return h.Value }

type hidden struct{}

func (hidden) Secret() int    { return 1 }
func (h *hidden) Tweak(n int) {}

// Unexported embed (mirrors net.IPConn { conn }): emitted faithfully as
// `embed hidden`, with hidden's methods promoting at the correct depth.
type Host4 struct {
	hidden
}

// Unexported embed plus an exported field (mirrors testing.B { common; N int }):
// the embed is faithful and the exported field is kept alongside it.
type Host5 struct {
	hidden
	X int
}

// raw is an unexported NON-struct type, so embedding it is not representable as
// a faithful `embed`; Host6 stays `#[go(hidden_embed)]` (mirrors a debug/macho
// struct embedding LoadBytes, which is `[]byte`).
type raw []byte

func (raw) Len() int { return 0 }

type Host6 struct {
	raw
}
