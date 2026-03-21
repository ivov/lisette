package convert

import (
	"fmt"
	"go/types"
	"strings"
)

// ReturnsToLisette converts a Go function's return types to Lisette.
// The qualifiedName is used for config lookups (e.g. "LookupEnv" or "Rat.Float64").
func ReturnsToLisette(signature *types.Signature, conv *Converter, qualifiedName string) TypeResult {
	return returnsToLisetteRecursive(signature, make(map[types.Type]bool), conv, qualifiedName)
}

func returnsToLisetteRecursive(signature *types.Signature, seen map[types.Type]bool, conv *Converter, qualifiedName string) TypeResult {
	results := signature.Results()

	if results.Len() == 0 {
		return TypeResult{LisetteType: "()"}
	}

	if results.Len() == 1 {
		if isErrorType(results.At(0).Type()) {
			if conv.cfg != nil && conv.cfg.HasDirectError(conv.currentPkgPath, qualifiedName) {
				return TypeResult{LisetteType: "error"}
			}
			if conv.cfg != nil && conv.cfg.HasNilableError(conv.currentPkgPath, qualifiedName) {
				return TypeResult{LisetteType: "Option<error>"}
			}
			if looksLikeNilableError(qualifiedName, signature) {
				return TypeResult{LisetteType: "Option<error>"}
			}
			return TypeResult{LisetteType: "Result<(), error>"}
		}
		// *T where T implements error → treat as error value
		if isPointerToErrorImpl(results.At(0).Type()) {
			return TypeResult{LisetteType: "error", IsDirectError: true}
		}
		return toLisetteRecursive(results.At(0).Type(), seen, conv)
	}

	last := results.At(results.Len() - 1)

	if isErrorType(last.Type()) {
		inner := collectReturnTypes(results, 0, results.Len()-1, seen, conv)
		innerType := inner.LisetteType
		if inner.SkipReason != nil {
			innerType = "Unknown"
		}
		return TypeResult{
			LisetteType: fmt.Sprintf("Result<%s, error>", innerType),
			SkipReason:  inner.SkipReason,
		}
	}

	// (T, bool) -> Option<T> when bool indicates presence/success
	if isBoolType(last.Type()) {
		if shouldConvertToOption(last.Name(), conv, qualifiedName) {
			nilable := results.Len() == 2 && isNilableGoType(results.At(0).Type())
			inner := collectReturnTypes(results, 0, results.Len()-1, seen, conv)
			innerType := inner.LisetteType
			if inner.SkipReason != nil {
				innerType = "Unknown"
			}
			return TypeResult{
				LisetteType: fmt.Sprintf("Option<%s>", innerType),
				SkipReason:  inner.SkipReason,
				CommaOk:     nilable,
			}
		}
	}

	return collectReturnTypes(results, 0, results.Len(), seen, conv)
}

// shouldConvertToOption determines if a (T, bool) return should become Option<T>.
func shouldConvertToOption(boolName string, conv *Converter, qualifiedName string) bool {
	if conv.cfg != nil && conv.cfg.HasBoolAsFlag(conv.currentPkgPath, qualifiedName) {
		return false
	}

	switch boolName {
	case "ok", "found", "present", "exists", "valid":
		return true
	}

	switch boolName {
	case "exact", "complete", "more", "loaded", "overflow", "underflow":
		return false
	}

	return true
}

func collectReturnTypes(results *types.Tuple, start, end int, seen map[types.Type]bool, conv *Converter) TypeResult {
	count := end - start

	if count == 0 {
		return TypeResult{LisetteType: "()"}
	}

	if count == 1 {
		return toLisetteRecursive(results.At(start).Type(), seen, conv)
	}

	var elems []string
	for i := start; i < end; i++ {
		elem := toLisetteRecursive(results.At(i).Type(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		elems = append(elems, elem.LisetteType)
	}

	return TypeResult{LisetteType: fmt.Sprintf("(%s)", strings.Join(elems, ", "))}
}

func isErrorType(t types.Type) bool {
	if named, ok := t.(*types.Named); ok {
		if named.Obj().Name() == "error" && named.Obj().Pkg() == nil {
			return true
		}
	}

	if iface, ok := t.Underlying().(*types.Interface); ok {
		return isErrorInterface(iface)
	}

	return false
}

// looksLikeNilableError returns true for methods like Err, Unwrap, and Cause
// that return nil when there is no error.
func looksLikeNilableError(qualifiedName string, sig *types.Signature) bool {
	if sig.Params().Len() != 0 {
		return false
	}
	return strings.HasSuffix(qualifiedName, ".Err") ||
		strings.HasSuffix(qualifiedName, ".Unwrap") ||
		strings.HasSuffix(qualifiedName, ".Cause")
}

func isBoolType(t types.Type) bool {
	if basic, ok := t.Underlying().(*types.Basic); ok {
		return basic.Kind() == types.Bool
	}
	return false
}

// isPointerToErrorImpl returns true if t is *T where T implements the error
// interface. These return `error` instead of `Ref<T>`.
func isPointerToErrorImpl(t types.Type) bool {
	ptr, ok := t.Underlying().(*types.Pointer)
	if !ok {
		return false
	}
	elem := ptr.Elem()
	named, ok := elem.(*types.Named)
	if !ok {
		return false
	}
	if _, isIface := named.Underlying().(*types.Interface); isIface {
		return false
	}
	errorIface := universeErrorInterface()
	if errorIface == nil {
		return false
	}
	return types.Implements(types.NewPointer(named), errorIface)
}

var cachedErrorIface *types.Interface
var cachedErrorIfaceOnce bool

func universeErrorInterface() *types.Interface {
	if cachedErrorIfaceOnce {
		return cachedErrorIface
	}
	cachedErrorIfaceOnce = true
	errorObj := types.Universe.Lookup("error")
	if errorObj == nil {
		return nil
	}
	cachedErrorIface, _ = errorObj.Type().Underlying().(*types.Interface)
	return cachedErrorIface
}
