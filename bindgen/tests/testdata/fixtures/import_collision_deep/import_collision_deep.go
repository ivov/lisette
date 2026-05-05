// Fixture: two imports with the same Go package name (`v1`) AND the same
// trailing two path segments (`meta/v1`). The simple `<parent>_<base>` alias
// would produce `meta_v1` for both, so bindgen must walk further up the path
// to keep aliases unique.
package import_collision_deep

import (
	bar_meta_v1 "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/import_collision_deep/bar/meta/v1"
	foo_meta_v1 "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/import_collision_deep/foo/meta/v1"
)

// Holder references both `meta/v1` packages.
type Holder struct {
	Foo *foo_meta_v1.FooMeta
	Bar *bar_meta_v1.BarMeta
}
