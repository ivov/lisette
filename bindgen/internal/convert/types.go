package convert

import (
	"fmt"
	"go/types"
	"slices"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

type SkipReason struct {
	Code           string
	Message        string
	EmitOpaqueType bool
}

type TypeResult struct {
	LisetteType          string
	SkipReason           *SkipReason
	CommaOk              bool // true when return type comes from a (T, bool) comma-ok pattern
	IsDirectError        bool // true when *T where T implements error was auto-detected as error value
	NilableReturnApplied bool // set when per-element wrapping already occurred; callers must skip whole-return wrap to avoid double-wrapping
}

func ToLisette(t types.Type, conv *Converter) TypeResult {
	return toLisetteRecursive(t, make(map[types.Type]bool), conv)
}

// ToLisetteNilable converts a Go type to Lisette, wrapping pointer and
// interface types in Option<>. Used for struct fields and collection elements
// where Go pointers/interfaces can be nil.
func ToLisetteNilable(t types.Type, conv *Converter) TypeResult {
	return toLisetteNilableRecursive(t, make(map[types.Type]bool), conv)
}

// convertParamType wraps a `*T` parameter in `Option<>` when the param's name
// appears in `nilable` — used for Go APIs where passing nil means "use default".
func convertParamType(t types.Type, name string, nilable []string, conv *Converter) TypeResult {
	if slices.Contains(nilable, name) {
		return ToLisetteNilable(t, conv)
	}
	return ToLisette(t, conv)
}

// isNilableGoType reports whether t is a Go-nilable type (pointer or non-empty non-error interface).
func isNilableGoType(t types.Type) bool {
	t = types.Unalias(t)
	switch t := t.(type) {
	case *types.Pointer:
		return true
	case *types.Interface:
		return !t.Empty() && !isErrorInterface(t)
	case *types.Named:
		switch u := t.Underlying().(type) {
		case *types.Pointer:
			return true
		case *types.Interface:
			return !u.Empty() && !isErrorInterface(u)
		}
	}
	return false
}
func toLisetteRecursive(t types.Type, seen map[types.Type]bool, conv *Converter) TypeResult {
	if seen[t] {
		return TypeResult{LisetteType: "Unknown"}
	}
	seen[t] = true
	defer delete(seen, t)

	switch t := t.(type) {
	case *types.Basic:
		return TypeResult{LisetteType: basicToLisette(t)}

	case *types.Slice:
		elem := toLisetteRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		return TypeResult{LisetteType: sliceOf(elem.LisetteType)}

	case *types.Array:
		elem := toLisetteRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		return TypeResult{LisetteType: arrayOf(elem.LisetteType, t.Len())}

	case *types.Map:
		key := toLisetteRecursive(t.Key(), seen, conv)
		if key.SkipReason != nil {
			return key
		}
		val := toLisetteRecursive(t.Elem(), seen, conv)
		if val.SkipReason != nil {
			return val
		}
		return TypeResult{LisetteType: mapOf(key.LisetteType, val.LisetteType)}

	case *types.Pointer:
		elem := toLisetteRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		return TypeResult{LisetteType: refOf(elem.LisetteType)}

	case *types.Chan:
		elem := toLisetteRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		switch t.Dir() {
		case types.SendRecv:
			return TypeResult{LisetteType: channelOf(elem.LisetteType)}
		case types.RecvOnly:
			return TypeResult{LisetteType: receiverOf(elem.LisetteType)}
		default: // types.SendOnly
			return TypeResult{LisetteType: senderOf(elem.LisetteType)}
		}

	case *types.Signature:
		return signatureToLisette(t, seen, conv)

	case *types.Interface:
		if t.Empty() {
			return TypeResult{LisetteType: "Unknown"}
		}
		if isErrorInterface(t) {
			return TypeResult{LisetteType: "error"}
		}
		return TypeResult{LisetteType: "Unknown"}

	case *types.Named:
		return namedToLisette(t, seen, conv)

	case *types.TypeParam:
		name := t.Obj().Name()
		if conv != nil && conv.typeParamSubstitutions != nil {
			if substituted, ok := conv.typeParamSubstitutions[name]; ok {
				return TypeResult{LisetteType: substituted}
			}
		}
		return TypeResult{LisetteType: name}

	case *types.Struct:
		if t.NumFields() == 0 {
			return TypeResult{LisetteType: "()"}
		}
		if conv == nil {
			return TypeResult{SkipReason: anonStructSkip()}
		}
		return conv.internAnonStruct(t)

	case *types.Alias:
		return toLisetteRecursive(t.Rhs(), seen, conv)

	default:
		return TypeResult{SkipReason: &SkipReason{
			Code:    "unknown-type",
			Message: fmt.Sprintf("unknown type: %T", t),
		}}
	}
}

// isScalarType returns true if *T should become Option<T> instead of
// Option<Ref<T>>. Excludes uint8/int8 (*byte is typically a raw C pointer).
func isScalarType(t types.Type) bool {
	basic, ok := t.Underlying().(*types.Basic)
	if !ok {
		return false
	}
	switch basic.Kind() {
	case types.Invalid, types.UnsafePointer, types.Uint8, types.Int8:
		return false
	default:
		return true
	}
}

func basicToLisette(t *types.Basic) string {
	switch t.Kind() {
	case types.Bool:
		return "bool"
	case types.Int:
		return "int"
	case types.Int8:
		return "int8"
	case types.Int16:
		return "int16"
	case types.Int32:
		if t.Name() == "rune" {
			return "rune"
		}
		return "int32"
	case types.Int64:
		return "int64"
	case types.Uint:
		return "uint"
	case types.Uint8:
		if t.Name() == "byte" {
			return "byte"
		}
		return "uint8"
	case types.Uint16:
		return "uint16"
	case types.Uint32:
		return "uint32"
	case types.Uint64:
		return "uint64"
	case types.Uintptr:
		return "uintptr"
	case types.Float32:
		return "float32"
	case types.Float64:
		return "float64"
	case types.Complex64:
		return "complex64"
	case types.Complex128:
		return "complex128"
	case types.String:
		return "string"
	case types.UnsafePointer:
		return "Unknown"
	default:
		return "Unknown"
	}
}

func signatureToLisette(signature *types.Signature, seen map[types.Type]bool, conv *Converter) TypeResult {
	var params []string

	param := signature.Params()
	for param := range param.Variables() {
		paramType := toLisetteRecursive(param.Type(), seen, conv)
		if paramType.SkipReason != nil {
			return paramType
		}
		params = append(params, paramType.LisetteType)
	}

	if signature.Variadic() && param.Len() > 0 {
		lastIdx := len(params) - 1
		params[lastIdx] = sliceToVarArgs(params[lastIdx])
	}

	returnType := "()"
	if signature.Results().Len() > 0 {
		ret := returnsToLisetteRecursive(signature, seen, conv, "")
		if ret.SkipReason != nil {
			return ret
		}
		returnType = ret.LisetteType
	}

	return TypeResult{LisetteType: fmt.Sprintf("fn(%s) -> %s", strings.Join(params, ", "), returnType)}
}

func namedToLisette(t *types.Named, seen map[types.Type]bool, conv *Converter) TypeResult {
	obj := t.Obj()
	pkg := obj.Pkg()

	if obj.Name() == "error" && pkg == nil {
		return TypeResult{LisetteType: "error"}
	}

	isExternal := false
	pkgPrefix := ""
	if pkg != nil && conv != nil && pkg.Path() != conv.currentPkgPath {
		if extract.IsInternalPackagePath(pkg.Path()) {
			return TypeResult{SkipReason: &SkipReason{
				Code:    "internal-package-ref",
				Message: fmt.Sprintf("references type from internal package %q", pkg.Path()),
			}}
		}
		isExternal = true
		// Sentinel-wrapped path; the emitter resolves it after collision detection.
		pkgPrefix = PkgRef(pkg.Path())
		conv.trackExternalPkg(pkg.Path(), pkg.Name())
	}

	if !isExternal && !obj.Exported() {
		if conv != nil && !conv.hasReachableUnexportedType(t) {
			return TypeResult{LisetteType: "Unknown"}
		}
		if namedImplementsError(t) {
			return TypeResult{LisetteType: "error"}
		}
		if conv != nil {
			if iface := conv.bestImplementedInterface(t); iface != nil {
				return toLisetteRecursive(iface, seen, conv)
			}
		}
		if s, ok := t.Underlying().(*types.Struct); ok && s.NumFields() > 0 {
			return TypeResult{SkipReason: &SkipReason{
				Code:    "opaque-unexported-struct",
				Message: fmt.Sprintf("underlying type %q is unexported", obj.Name()),
			}}
		}
		return toLisetteRecursive(t.Underlying(), seen, conv)
	}

	typeName := obj.Name()

	typeArgs := t.TypeArgs()
	if typeArgs != nil && typeArgs.Len() > 0 {
		var args []string
		for arg := range typeArgs.Types() {
			result := toLisetteRecursive(arg, seen, conv)
			if result.SkipReason != nil {
				return result
			}
			args = append(args, result.LisetteType)
		}
		if isExternal {
			typeName = pkgPrefix + "." + obj.Name()
		} else if pkg != nil {
			typeName = SelfQualify(pkg.Name(), obj.Name(), t.Origin().TypeParams().Len())
		}
		return TypeResult{LisetteType: fmt.Sprintf("%s<%s>", typeName, strings.Join(args, ", "))}
	}

	if pkg == nil {
		return TypeResult{LisetteType: obj.Name()}
	}

	if isExternal {
		return TypeResult{LisetteType: pkgPrefix + "." + obj.Name()}
	}

	return TypeResult{LisetteType: typeName}
}

var preludeGenericArity = map[string]int{"Option": 1, "Result": 2, "Partial": 2}

// CollidesWithPreludeGeneric reports whether a Go type's name and arity match a
// prelude generic, meaning references to it must be package-qualified.
func CollidesWithPreludeGeneric(typeName string, arity int) bool {
	a, ok := preludeGenericArity[typeName]
	return ok && a == arity
}

// SelfQualify returns `pkgName.typeName` when a package's own type collides with
// a prelude generic by name and arity (e.g. huh's `Option<T>`), so the reference
// stays distinct from the prelude. The declaration itself stays bare.
func SelfQualify(pkgName, typeName string, arity int) string {
	if CollidesWithPreludeGeneric(typeName, arity) {
		return pkgName + "." + typeName
	}
	return typeName
}

// wrapOption wraps a converted type in `Option<...>`, propagating SkipReason.
func wrapOption(r TypeResult) TypeResult {
	if r.SkipReason != nil {
		return r
	}
	return TypeResult{LisetteType: optionOf(r.LisetteType)}
}

// toLisetteNilableRecursive converts a Go type to Lisette in a nilable context.
// Pointers become Option<Ref<T>>, named non-error interfaces become Option<Name>,
// and function types (including named func aliases) become Option<fn(...)> /
// Option<Name> — Go func values are nilable, and zero-value (nil) is meaningful
// in struct literals.
// The nilable flag propagates into collection element types (Slice, Map values).
func toLisetteNilableRecursive(t types.Type, seen map[types.Type]bool, conv *Converter) TypeResult {
	switch t := t.(type) {
	case *types.Pointer:
		elem := toLisetteRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		if isScalarType(t.Elem()) {
			return TypeResult{LisetteType: optionOf(elem.LisetteType)}
		}
		return TypeResult{LisetteType: optionOf(refOf(elem.LisetteType))}

	case *types.Signature:
		return wrapOption(toLisetteRecursive(t, seen, conv))

	case *types.Named:
		switch u := t.Underlying().(type) {
		case *types.Interface:
			if !u.Empty() && !isErrorInterface(u) {
				return wrapOption(namedToLisette(t, seen, conv))
			}
		case *types.Signature:
			return wrapOption(namedToLisette(t, seen, conv))
		}
		return namedToLisette(t, seen, conv)

	case *types.Slice:
		elem := toLisetteNilableRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		return TypeResult{LisetteType: sliceOf(elem.LisetteType)}

	case *types.Array:
		elem := toLisetteNilableRecursive(t.Elem(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		return TypeResult{LisetteType: arrayOf(elem.LisetteType, t.Len())}

	case *types.Map:
		key := toLisetteRecursive(t.Key(), seen, conv)
		if key.SkipReason != nil {
			return key
		}
		val := toLisetteNilableRecursive(t.Elem(), seen, conv)
		if val.SkipReason != nil {
			return val
		}
		return TypeResult{LisetteType: mapOf(key.LisetteType, val.LisetteType)}

	case *types.Alias:
		return toLisetteNilableRecursive(t.Rhs(), seen, conv)

	default:
		return toLisetteRecursive(t, seen, conv)
	}
}

func namedImplementsError(t *types.Named) bool {
	errorIface := universeErrorInterface()
	if errorIface == nil {
		return false
	}
	return types.Implements(t, errorIface) || types.Implements(types.NewPointer(t), errorIface)
}

func isErrorInterface(_interface *types.Interface) bool {
	if _interface.NumMethods() != 1 {
		return false
	}

	method := _interface.Method(0)
	if method.Name() != "Error" {
		return false
	}

	signature, ok := method.Type().(*types.Signature)
	if !ok {
		return false
	}

	if signature.Params().Len() != 0 {
		return false
	}

	if signature.Results().Len() != 1 {
		return false
	}

	returnType, ok := signature.Results().At(0).Type().(*types.Basic)
	if !ok {
		return false
	}

	return returnType.Kind() == types.String
}
