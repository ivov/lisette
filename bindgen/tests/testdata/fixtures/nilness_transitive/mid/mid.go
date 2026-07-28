// Middle hop: forwards leaf results so the root sits two package hops from
// the nil source.
package mid

import (
	"github.com/ivov/lisette/bindgen/tests/testdata/fixtures/nilness_transitive/mid/leaf"
)

type Thing = leaf.Thing

func MakeIt() *leaf.Thing { return leaf.NewNil() }

func MakeItHonest() *leaf.Thing { return leaf.FetchNil() }
