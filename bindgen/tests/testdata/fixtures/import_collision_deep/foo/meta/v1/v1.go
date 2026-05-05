// Subpackage whose Go name (`v1`) and trailing two segments (`meta/v1`)
// collide with another subpackage in this fixture.
package v1

// FooMeta is a marker type from foo/meta/v1.
type FooMeta struct {
	Name string
}
