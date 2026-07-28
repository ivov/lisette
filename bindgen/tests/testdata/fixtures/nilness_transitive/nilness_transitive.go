// Fixture: definite-nil facts must propagate through calls, at any package
// depth, and veto constructor-name heuristics.
package nilness_transitive

import (
	"github.com/ivov/lisette/bindgen/tests/testdata/fixtures/nilness_transitive/mid"
)

type Gizmo struct{ ID int }

func helperNil() *Gizmo { return nil }

func NewViaHelper() *Gizmo { return helperNil() }

func GrabViaHelper() *Gizmo { return helperNil() }

func NewDirectNil() *Gizmo { return nil }

func NewBuilt() *Gizmo { return &Gizmo{} }

func UnnamedBuilt() *Gizmo {
	g := &Gizmo{}
	return g
}

type Box[T any] struct{ Value T }

func NewBox[T any]() *Box[T] { return &Box[T]{} }

func NewBoxInt() *Box[int] { return NewBox[int]() }

func Grab() *mid.Thing { return mid.MakeIt() }

func GrabHonest() *mid.Thing { return mid.MakeItHonest() }
