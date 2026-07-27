// Fixture: nil-return witnesses guarded on a parameter are discharged by
// provably non-nil arguments, while undischargeable witnesses beat
// constructor names.
package nilness_guarded_witness

type Gadget struct{ ID int }

func NewGadget(id int) *Gadget {
	if id < 0 {
		return nil
	}
	return &Gadget{ID: id}
}

func ensure(g *Gadget) *Gadget {
	if g == nil {
		return nil
	}
	return g
}

func Prepared() *Gadget {
	return ensure(&Gadget{})
}

func Passthrough(g *Gadget) *Gadget {
	return ensure(g)
}
