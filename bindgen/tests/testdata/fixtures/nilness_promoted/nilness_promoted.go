// Fixture: promoted methods from another package get body analysis, so a
// provably-nil "With"-prefixed method emits Option and a provably-non-nil
// method emits Ref.
package nilness_promoted

import (
	"github.com/ivov/lisette/bindgen/tests/testdata/fixtures/nilness_promoted/inner"
)

type Wrapper struct {
	inner.Core
}
