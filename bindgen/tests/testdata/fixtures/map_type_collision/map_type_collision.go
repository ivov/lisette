// Fixture: a generic `Map` collides with the built-in, so its references qualify
// while a genuine `map[string]int` stays the bare `Map<string, int>`.
package map_type_collision

type Map[K comparable, V any] struct {
	Keys   []K
	Values []V
}

func (m Map[K, V]) Len() int { return len(m.Keys) }

func NewMap[K comparable, V any]() Map[K, V] { return Map[K, V]{} }

type Holder struct {
	Index  Map[string, int]
	Counts map[string]int
}

func Counts() map[string]int { return nil }
