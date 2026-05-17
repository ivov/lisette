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
