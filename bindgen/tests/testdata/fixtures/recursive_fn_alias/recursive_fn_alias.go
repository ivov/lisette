// Function-type aliases that recursively reference themselves cannot be
// emitted as transparent Lisette aliases (Lisette rejects self-referential
// type aliases). They must fall back to opaque types so the package compiles
// and downstream signatures still resolve.
package recursivefnalias

type Controller struct {
	Name string
}

// Filter is a self-referential function type — the classic Go middleware
// chain pattern (revel, negroni, chi, etc.).
type Filter func(c *Controller, fc []Filter)

// FilterEq exists to confirm functions referencing the recursive type still
// emit valid signatures after the fallback.
func FilterEq(a, b Filter) bool { return false }

// FilterChain confirms slice-of-self also works at the call boundary.
func FilterChain(filters []Filter) {}

// Sibling references with `Filter` as a substring must NOT trigger the
// recursion detector — `FilterMatcher` is unrelated to `Filter`.
type FilterMatcher func(name string) bool

// HandlesFilter confirms that the substring check uses identifier boundaries:
// `Filter` here is a parameter type, not a self-reference of `HandlesFilter`.
type HandlesFilter func(f Filter)
