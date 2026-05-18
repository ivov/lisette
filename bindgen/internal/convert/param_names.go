package convert

import (
	"fmt"
	"go/types"
	"strings"
	"unicode"
)

func collectNamedParams(params *types.Tuple) map[string]int {
	used := make(map[string]int, params.Len())
	for v := range params.Variables() {
		if n := v.Name(); n != "" {
			used[sanitizeParamName(n)]++
		}
	}
	return used
}

// deriveParamName synthesizes a parameter name from the Go type when the
// original signature has none.
func deriveParamName(t types.Type, index int, variadic bool, used map[string]int) string {
	base := derivedBaseName(t, variadic)
	if base == "" {
		base = fmt.Sprintf("arg%d", index)
	}
	base = sanitizeParamName(base)
	used[base]++
	if used[base] == 1 {
		return base
	}
	return fmt.Sprintf("%s%d", base, used[base])
}

func derivedBaseName(t types.Type, variadic bool) string {
	if variadic {
		if slice, ok := t.(*types.Slice); ok {
			return pluralizeName(elementLongName(slice.Elem()))
		}
	}
	return scalarName(t)
}

// elementLongName is like scalarName but uses the full type name for
// primitives ("string" rather than "s") so pluralization reads naturally.
// `VarArgs<string>` becomes "strings", `Slice<int>` becomes "ints".
func elementLongName(t types.Type) string {
	switch x := t.(type) {
	case *types.Basic:
		return x.Name()
	case *types.Named:
		if special := specialNamedShortName(x.Obj()); special != "" {
			return special
		}
		return lowerFirst(x.Obj().Name())
	case *types.TypeParam:
		return lowerFirst(x.Obj().Name())
	case *types.Pointer:
		return elementLongName(x.Elem())
	case *types.Alias:
		if special := specialNamedShortName(x.Obj()); special != "" {
			return special
		}
		if name := lowerFirst(x.Obj().Name()); name != "" {
			return name
		}
		return elementLongName(types.Unalias(x))
	}
	return scalarName(t)
}

func scalarName(t types.Type) string {
	switch x := t.(type) {
	case *types.Pointer:
		return scalarName(x.Elem())
	case *types.Alias:
		if special := specialNamedShortName(x.Obj()); special != "" {
			return special
		}
		if name := lowerFirst(x.Obj().Name()); name != "" {
			return name
		}
		return scalarName(types.Unalias(x))
	case *types.Named:
		obj := x.Obj()
		if special := specialNamedShortName(obj); special != "" {
			return special
		}
		return lowerFirst(obj.Name())
	case *types.TypeParam:
		return lowerFirst(x.Obj().Name())
	case *types.Slice:
		if isByteLike(x.Elem()) {
			return "b"
		}
		return pluralizeName(elementLongName(x.Elem()))
	case *types.Array:
		if isByteLike(x.Elem()) {
			return "b"
		}
		return pluralizeName(elementLongName(x.Elem()))
	case *types.Map:
		return "m"
	case *types.Chan:
		return "ch"
	case *types.Signature:
		return "f"
	case *types.Basic:
		return basicScalarName(x)
	case *types.Interface:
		if x.Empty() {
			return "v"
		}
	}
	return ""
}

func basicScalarName(b *types.Basic) string {
	switch b.Kind() {
	case types.Bool, types.UntypedBool:
		return "b"
	case types.String, types.UntypedString:
		return "s"
	case types.Int, types.Int8, types.Int16, types.Int32, types.Int64,
		types.Uint, types.Uint8, types.Uint16, types.Uint32, types.Uint64,
		types.Uintptr, types.UntypedInt, types.UntypedRune:
		return "n"
	case types.Float32, types.Float64, types.UntypedFloat:
		return "f"
	case types.Complex64, types.Complex128, types.UntypedComplex:
		return "c"
	}
	return ""
}

func specialNamedShortName(obj *types.TypeName) string {
	if obj == nil {
		return ""
	}
	pkg := obj.Pkg()
	name := obj.Name()
	if pkg == nil {
		if name == "error" {
			return "err"
		}
		return ""
	}
	switch pkg.Path() + "." + name {
	case "context.Context":
		return "ctx"
	case "time.Time":
		return "t"
	case "time.Duration":
		return "d"
	case "github.com/google/uuid.UUID":
		return "id"
	}
	return ""
}

func isByteLike(t types.Type) bool {
	switch x := t.(type) {
	case *types.Basic:
		return x.Kind() == types.Byte || x.Kind() == types.Uint8
	case *types.Named:
		return x.Obj().Name() == "byte"
	case *types.Alias:
		return isByteLike(types.Unalias(x))
	}
	return false
}

func lowerFirst(s string) string {
	if s == "" {
		return ""
	}
	runes := []rune(s)
	runes[0] = unicode.ToLower(runes[0])
	return string(runes)
}

func pluralizeName(s string) string {
	if s == "" {
		return ""
	}
	if strings.HasSuffix(s, "s") {
		return s
	}
	return s + "s"
}
