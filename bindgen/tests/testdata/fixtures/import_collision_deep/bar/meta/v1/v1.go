// Subpackage whose Go name (`v1`) and trailing two segments (`meta/v1`)
// collide with another subpackage in this fixture.
package v1

// BarMeta is a marker type from bar/meta/v1.
type BarMeta struct {
	Name string
}
