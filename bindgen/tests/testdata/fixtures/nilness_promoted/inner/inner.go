// Embedded source type with one provably-non-nil and one provably-nil
// method. A named non-struct type embeds unfaithfully, so the wrapper
// promotes these methods explicitly.
package inner

type Core []int

func (c *Core) Produce() *Core { return &Core{} }

func (c *Core) Withhold() *Core { return nil }
