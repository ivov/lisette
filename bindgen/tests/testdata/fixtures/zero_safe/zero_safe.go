// Fixture for the zero_safe override: identically-shaped types differ only by
// whether bindgen.json curates them, since shape never implies zero-safety.
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

func (c *Counter) Get() int { return c.n }

func (h *Handle) Get() int { return *h.p }

func (v *Verified) Get() int { return v.n }

func (p *PartiallyHidden) Get() int { return *p.p }

func (v *VerifiedPartiallyHidden) Get() int { return *v.p }
