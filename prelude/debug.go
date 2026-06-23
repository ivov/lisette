package lisette

import (
	"fmt"
	"reflect"
	"sort"
	"strconv"
	"strings"
)

// Debugger renders a value for diagnostics, quoting nested strings (unlike Stringer's display form).
type Debugger interface {
	DebugString() string
}

// Debug renders v for a diagnostic: strings quoted, a Debugger via DebugString,
// slices and maps element-wise, else display form.
func Debug(v any) string {
	if s, ok := v.(string); ok {
		return strconv.Quote(s)
	}

	rv := reflect.ValueOf(v)
	if rv.Kind() == reflect.Pointer && rv.IsNil() {
		return "nil"
	}
	if d, ok := v.(Debugger); ok {
		return d.DebugString()
	}

	switch rv.Kind() {
	case reflect.Slice, reflect.Array:
		parts := make([]string, rv.Len())
		for i := range parts {
			parts[i] = Debug(rv.Index(i).Interface())
		}
		return "[" + strings.Join(parts, ", ") + "]"
	case reflect.Map:
		if rv.Len() == 0 {
			return "{}"
		}
		parts := make([]string, 0, rv.Len())
		for _, key := range rv.MapKeys() {
			parts = append(parts, Debug(key.Interface())+": "+Debug(rv.MapIndex(key).Interface()))
		}
		sort.Strings(parts)
		return "{ " + strings.Join(parts, ", ") + " }"
	case reflect.Pointer:
		return Debug(rv.Elem().Interface())
	}
	return fmt.Sprintf("%v", v)
}
