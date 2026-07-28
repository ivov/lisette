// Fixture: unexported package singletons prove non-nil when the declared
// initializer is non-nil, no store anywhere can write nil, and the address
// never escapes. Exported globals and store-only vars stay Option.
package nilness_singletons

type Registry struct{ entries map[string]int }

var defaultRegistry = &Registry{}

var Shared = &Registry{}

var lazy *Registry

func Default() *Registry { return defaultRegistry }

func SharedRegistry() *Registry { return Shared }

func Lazy() *Registry { return lazy }

func Enable() {
	lazy = &Registry{}
}
