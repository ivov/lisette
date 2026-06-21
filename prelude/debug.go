package lisette

import (
	"fmt"
	"strconv"
)

// Debugger renders a value for diagnostics, quoting nested strings (unlike Stringer's display form).
type Debugger interface {
	DebugString() string
}

// Debug renders v for a diagnostic: strings quoted, a Debugger via DebugString, else display form.
func Debug(v any) string {
	switch x := v.(type) {
	case string:
		return strconv.Quote(x)
	case Debugger:
		return x.DebugString()
	default:
		return fmt.Sprintf("%v", x)
	}
}
