// Package v2 sits at a `/v2` path but is genuinely named `v2` (like the
// `k8s.io/api/.../v2` packages), so bindgen must emit an explicit
// `// Package: v2` directive even though it matches the last path segment.
package v2

// Config is a marker type to give the package an exported surface.
type Config struct {
	Name string
}
