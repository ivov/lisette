// Fixture: an interface with type terms is a constraint only, never a value type.
package constraint_interfaces

type Label interface {
	~string
	Display() string
}

type Key interface {
	comparable
	Hash() uint64
}

type Number interface {
	~int | ~float64
}

// The error method set renders as an `error` newtype, which keeps the marker.
type ErrorKey interface {
	error
	~int
}

// The alias is a constraint as well.
type KeyAlias = Key

func Show[T Label](label T) string { return label.Display() }

func Lookup[K Key](keys []K, key K) bool {
	for _, k := range keys {
		if k == key {
			return true
		}
	}
	return false
}

func Sum[T Number](values []T) T {
	var total T
	for _, v := range values {
		total += v
	}
	return total
}
