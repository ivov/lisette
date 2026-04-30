package convert

import (
	"fmt"
	"go/importer"
	"go/types"
	"strings"
	"sync"
)

// ReturnsToLisette converts a Go function's return types to Lisette.
// The qualifiedName is used for config lookups (e.g. "LookupEnv" or "Rat.Float64").
func ReturnsToLisette(signature *types.Signature, conv *Converter, qualifiedName string) TypeResult {
	return returnsToLisetteRecursive(signature, make(map[types.Type]bool), conv, qualifiedName)
}

func returnElementToLisette(t types.Type, seen map[types.Type]bool, conv *Converter) TypeResult {
	if arr := unwrapArray(t); arr != nil {
		return arrayReturnTypeResult(arr, seen, conv)
	}
	return toLisetteRecursive(t, seen, conv)
}

// maybeWrapNilableFunction wraps function-typed returns in Option.
//
// Bare signatures are wrapped by default (opt-out via non_nilable_return).
// Named function types are unwrapped by default (opt-in via nilable_return).
// The second return is true when wrapping occurred, signalling callers to skip
// the outer Option wrap in converters.go and avoid double-wrapping.
func maybeWrapNilableFunction(t types.Type, lisetteType string, conv *Converter, qualifiedName string) (string, bool) {
	switch t.(type) {
	case *types.Signature:
		if conv != nil && conv.cfg != nil && conv.cfg.IsNonNilableReturn(conv.currentPkgPath, qualifiedName) {
			return lisetteType, false
		}
		return fmt.Sprintf("Option<%s>", lisetteType), true
	case *types.Named:
		if _, ok := t.Underlying().(*types.Signature); !ok {
			return lisetteType, false
		}
		if conv != nil && conv.cfg != nil && conv.cfg.ShouldWrapNilableReturn(conv.currentPkgPath, qualifiedName) {
			return fmt.Sprintf("Option<%s>", lisetteType), true
		}
		return lisetteType, false
	default:
		return lisetteType, false
	}
}

func returnsToLisetteRecursive(signature *types.Signature, seen map[types.Type]bool, conv *Converter, qualifiedName string) TypeResult {
	results := signature.Results()

	if results.Len() == 0 {
		if conv != nil && conv.cfg != nil && conv.cfg.IsNeverReturn(conv.currentPkgPath, qualifiedName) {
			return TypeResult{LisetteType: "Never"}
		}
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
		elem := returnElementToLisette(results.At(0).Type(), seen, conv)
		if elem.SkipReason == nil {
			wrapped, applied := maybeWrapNilableFunction(results.At(0).Type(), elem.LisetteType, conv, qualifiedName)
			elem.LisetteType = wrapped
			if applied {
				elem.NilableReturnApplied = true
			}
		}
		return elem
	}

	last := results.At(results.Len() - 1)

	if isErrorType(last.Type()) {
		inner := collectReturnTypes(results, 0, results.Len()-1, seen, conv, qualifiedName)
		innerType := inner.LisetteType
		if inner.SkipReason != nil {
			innerType = "Unknown"
		}
		if (conv.cfg != nil && conv.cfg.IsPartialResult(conv.currentPkgPath, qualifiedName)) ||
			isPartialIOMethod(signature, qualifiedName) {
			return TypeResult{
				LisetteType:          fmt.Sprintf("Partial<%s, error>", innerType),
				SkipReason:           inner.SkipReason,
				NilableReturnApplied: inner.NilableReturnApplied,
			}
		}
		return TypeResult{
			LisetteType:          fmt.Sprintf("Result<%s, error>", innerType),
			SkipReason:           inner.SkipReason,
			NilableReturnApplied: inner.NilableReturnApplied,
		}
	}

	// (T, bool) -> Option<T> when bool indicates presence/success
	if isBoolType(last.Type()) {
		if shouldConvertToOption(last.Name(), conv, qualifiedName) {
			nilable := results.Len() == 2 && isNilableGoType(results.At(0).Type())
			inner := collectReturnTypes(results, 0, results.Len()-1, seen, conv, qualifiedName)
			innerType := inner.LisetteType
			if inner.SkipReason != nil {
				innerType = "Unknown"
			}
			return TypeResult{
				LisetteType:          fmt.Sprintf("Option<%s>", innerType),
				SkipReason:           inner.SkipReason,
				CommaOk:              nilable,
				NilableReturnApplied: inner.NilableReturnApplied,
			}
		}
	}

	return collectReturnTypes(results, 0, results.Len(), seen, conv, qualifiedName)
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

// maxReturnTupleArity mirrors MAX_TUPLE_ARITY in
// crates/syntax/src/parse/mod.rs.
const maxReturnTupleArity = 5

func collectReturnTypes(results *types.Tuple, start, end int, seen map[types.Type]bool, conv *Converter, qualifiedName string) TypeResult {
	count := end - start

	if count == 0 {
		return TypeResult{LisetteType: "()"}
	}

	if count == 1 {
		elem := returnElementToLisette(results.At(start).Type(), seen, conv)
		if elem.SkipReason == nil {
			wrapped, applied := maybeWrapNilableFunction(results.At(start).Type(), elem.LisetteType, conv, qualifiedName)
			elem.LisetteType = wrapped
			if applied {
				elem.NilableReturnApplied = true
			}
		}
		return elem
	}

	if count > maxReturnTupleArity {
		return TypeResult{SkipReason: &SkipReason{
			Code:    "tuple-too-large",
			Message: fmt.Sprintf("%d-element return tuple exceeds Lisette's %d-element limit", count, maxReturnTupleArity),
		}}
	}

	var elems []string
	anyApplied := false
	for i := start; i < end; i++ {
		elem := returnElementToLisette(results.At(i).Type(), seen, conv)
		if elem.SkipReason != nil {
			return elem
		}
		wrapped, applied := maybeWrapNilableFunction(results.At(i).Type(), elem.LisetteType, conv, qualifiedName)
		if applied {
			anyApplied = true
		}
		elems = append(elems, wrapped)
	}

	return TypeResult{
		LisetteType:          fmt.Sprintf("(%s)", strings.Join(elems, ", ")),
		NilableReturnApplied: anyApplied,
	}
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

var errorIfaceOnce sync.Once
var cachedErrorIface *types.Interface

func universeErrorInterface() *types.Interface {
	errorIfaceOnce.Do(func() {
		errorObj := types.Universe.Lookup("error")
		if errorObj == nil {
			return
		}
		cachedErrorIface, _ = errorObj.Type().Underlying().(*types.Interface)
	})
	return cachedErrorIface
}

// partialIOMethod maps io interface names to their methods that return
// non-exclusive (T, error) results.
var partialIOMethods = map[string]string{
	"io.Reader":   "Read",
	"io.Writer":   "Write",
	"io.ReaderAt": "ReadAt",
	"io.WriterAt": "WriteAt",
}

// isPartialIOMethod returns true if the method's receiver type implements
// io.Reader, io.Writer, io.ReaderAt, or io.WriterAt, and the method is the
// corresponding interface method. These methods return (T, error) where both
// values may be simultaneously meaningful.
func isPartialIOMethod(signature *types.Signature, qualifiedName string) bool {
	recv := signature.Recv()
	if recv == nil {
		return false
	}

	dot := strings.IndexByte(qualifiedName, '.')
	if dot < 0 {
		return false
	}
	methodName := qualifiedName[dot+1:]

	recvType := recv.Type()
	if ptr, ok := recvType.(*types.Pointer); ok {
		recvType = ptr.Elem()
	}
	named, ok := recvType.(*types.Named)
	if !ok {
		return false
	}

	for ifacePath, ifaceMethod := range partialIOMethods {
		if methodName != ifaceMethod {
			continue
		}
		iface := lookupIOInterface(ifacePath)
		if iface == nil {
			continue
		}
		if types.Implements(named, iface) || types.Implements(types.NewPointer(named), iface) {
			return true
		}
	}

	return false
}

var cachedIOInterfaces sync.Map

func lookupIOInterface(qualifiedName string) *types.Interface {
	if val, ok := cachedIOInterfaces.Load(qualifiedName); ok {
		return val.(*types.Interface)
	}

	dot := strings.LastIndexByte(qualifiedName, '.')
	if dot < 0 {
		return nil
	}
	pkgPath := qualifiedName[:dot]
	name := qualifiedName[dot+1:]

	pkg, err := importer.Default().Import(pkgPath)
	if err != nil {
		return nil
	}

	obj := pkg.Scope().Lookup(name)
	if obj == nil {
		return nil
	}

	iface, _ := obj.Type().Underlying().(*types.Interface)
	if iface != nil {
		cachedIOInterfaces.Store(qualifiedName, iface)
	}
	return iface
}
