// Fixture: stdlib `log` collides with a longer external path also named `log`.
// The longer path must be aliased so the typedef has two distinct imports.
package import_collision_stdlib

import (
	stdlog "log"

	extlog "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/import_collision_stdlib/sublog"
)

// StdLogger returns a value from stdlib log.
func StdLogger() *stdlog.Logger { return nil }

// ExtEntry returns a value from the external `log` package.
func ExtEntry() *extlog.Entry { return nil }

// Holder references both `log` packages.
type Holder struct {
	Std *stdlog.Logger
	Ext *extlog.Entry
}
