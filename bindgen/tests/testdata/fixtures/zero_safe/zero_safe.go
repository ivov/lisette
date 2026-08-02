// Fixture for the zero curation overrides: identically-shaped types differ
// only by what bindgen.json curates, since shape never implies zero safety.
// Opaque types are refused at zero unless zero_safe admits them, while
// partially-hidden structs are admitted unless zero_unsafe denies them.
package zero_safe

type Counter struct{ n int }

type Handle struct{ p *int }

type Verified struct{ n int }

type PartiallyHidden struct {
	Label string
	p     *int
}

type VerifiedPartiallyHidden struct {
	Label string
	p     *int
}

type BrokenPartiallyHidden struct {
	Label string
	p     *int
}

func (c *Counter) Get() int { return c.n }

func (h *Handle) Get() int { return *h.p }

func (v *Verified) Get() int { return v.n }

func (p *PartiallyHidden) Get() int { return *p.p }

func (v *VerifiedPartiallyHidden) Get() int { return *v.p }

func (b *BrokenPartiallyHidden) Get() int { return *b.p }
