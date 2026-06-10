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

// Unexported embed (mirrors net.IPConn { conn }): its promoted methods stay
// flattened on Host4 rather than be dropped.
type Host4 struct {
	hidden
}

// Unexported embed plus an exported field (mirrors testing.B { common; N int }):
// a visible Record that must stay unembeddable (marked), not mistaken for flat.
type Host5 struct {
	hidden
	X int
}
