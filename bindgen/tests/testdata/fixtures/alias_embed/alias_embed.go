package alias_embed

type Base struct {
	X int
}

func (Base) M() int { return 0 }

type Alias = Base

// Host embeds the alias Alias (= Base): the embed keeps the alias name (h.Alias, not h.Base).
type Host struct {
	Alias
}
