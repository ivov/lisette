package sealed_internal

import "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/sealed_internal/internal/surface"

// Content is sealed by a parameter from an internal package.
type Content interface {
	AttachContent(conn *surface.Conn)
	Title() string
}

// Surfaced is sealed by a result from an internal package.
type Surfaced interface {
	Surface() *surface.Conn
}

// Generic stays opaque: a type parameter has no exact seal identity.
type Generic[T any] interface {
	AttachContent(conn *surface.Conn, value T)
}

// Hidden stays opaque: an unexported field has no exact seal identity.
type Hidden interface {
	AttachContent(opts struct{ conn *surface.Conn })
}

// Options holds a Content.
type Options struct {
	Content Content
}

// Show accepts any Content.
func Show(c Content) {}
