package unexported_generic

// inner is an unexported GENERIC type embedded by exported types at different
// instantiations; it must not be emitted as one faithful `embed inner`. Generic
// unexported embeds fall back to flatten + hidden_embed.
type inner[T any] struct{ Value T }

func (i inner[T]) Get() T { return i.Value }

type OuterInt struct{ inner[int] }
type OuterString struct{ inner[string] }
