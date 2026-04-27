// Fixture: two imports with the same package name (`template`) but different
// paths must be aliased on the Lisette side so the typedef resolves cleanly.
package import_collision

import (
	htemplate "html/template"
	ttemplate "text/template"
)

// HTMLTmpl returns a value from html/template.
func HTMLTmpl() *htemplate.Template { return nil }

// TextTmpl returns a value from text/template.
func TextTmpl() *ttemplate.Template { return nil }

// Holder references both html/template and text/template.
type Holder struct {
	HTML *htemplate.Template
	Text *ttemplate.Template
}
