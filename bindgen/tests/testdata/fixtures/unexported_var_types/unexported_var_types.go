// Fixture: a var whose Go type is unexported is read-only, since a write of the rendered type would not build.
package unexported_var_types

type Order interface {
	Name() string
}

type bigOrder struct{}

func (bigOrder) Name() string { return "big" }

type mode int

// Big renders as the interface it implements.
var Big bigOrder

// Fast renders as the underlying int.
var Fast mode = 1

// Shared renders as a reference to the interface its pointed-to type implements.
var Shared = &bigOrder{}

// Current has an exported type, so it stays writable.
var Current Order = Big
