package ui

import "github.com/ivov/lisette/bindgen/tests/testdata/fixtures/sealed_internal/internal/surface"

// View implements Content and Surfaced without importing the root package.
type View struct {
	title string
	conn  *surface.Conn
}

// AttachContent connects the view to a window.
func (v *View) AttachContent(conn *surface.Conn) { v.conn = conn }

// Title returns the view title.
func (v *View) Title() string { return v.title }

// Surface returns the connection.
func (v View) Surface() *surface.Conn { return v.conn }

// AttachHidden is skipped: an unexported field has no exact seal identity.
func (v *View) AttachHidden(opts struct{ conn *surface.Conn }) { v.conn = opts.conn }

// New builds a view.
func New(title string) *View { return &View{title: title} }

// Box pairs the internal type with a type parameter.
type Box[T any] struct {
	conn *surface.Conn
}

// AttachContent is skipped: a type parameter has no exact seal identity.
func (b *Box[T]) AttachContent(conn *surface.Conn, value T) { b.conn = conn }
