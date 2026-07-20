package convert

import (
	"go/types"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

type TypeParamSpec struct {
	Name  string
	Bound string
}

type TypeParamSpecs []TypeParamSpec

// Used when a constraint is unrepresentable: callers still need the arity so
// opaque placeholders, dependent aliases, and impl heads stay in sync.
func bareTypeParamSpecs(tps *types.TypeParamList) TypeParamSpecs {
	if tps == nil || tps.Len() == 0 {
		return nil
	}
	specs := make(TypeParamSpecs, 0, tps.Len())
	for tp := range tps.TypeParams() {
		specs = append(specs, TypeParamSpec{Name: tp.Obj().Name()})
	}
	return specs
}

// `E: cmp.Ordered, V` for sites that introduce type parameters.
func (ps TypeParamSpecs) FormatDecl() string {
	if len(ps) == 0 {
		return ""
	}
	parts := make([]string, len(ps))
	for i, p := range ps {
		if p.Bound != "" {
			parts[i] = p.Name + ": " + p.Bound
		} else {
			parts[i] = p.Name
		}
	}
	return strings.Join(parts, ", ")
}

// `E, V` for sites that reference an already-introduced parameter.
func (ps TypeParamSpecs) FormatUse() string {
	if len(ps) == 0 {
		return ""
	}
	names := make([]string, len(ps))
	for i, p := range ps {
		names[i] = p.Name
	}
	return strings.Join(names, ", ")
}

// `<E: cmp.Ordered, V>` or empty when there are no type parameters.
func (ps TypeParamSpecs) DeclBlock() string {
	if len(ps) == 0 {
		return ""
	}
	return "<" + ps.FormatDecl() + ">"
}

// `<E, V>` or empty when there are no type parameters.
func (ps TypeParamSpecs) UseBlock() string {
	if len(ps) == 0 {
		return ""
	}
	return "<" + ps.FormatUse() + ">"
}

// Named-type identity is checked before .Underlying(), which discards the
// wrapper — afterwards cmp.Ordered's structural shape would short-circuit
// as plain `comparable`.
func recognizeBound(constraint types.Type, conv *Converter) (boundExpr string, ok bool) {
	if named, isNamed := constraint.(*types.Named); isNamed {
		obj := named.Obj()
		if obj.Pkg() != nil && obj.Pkg().Path() == "cmp" && obj.Name() == "Ordered" {
			return qualifyTypeNameBound(obj, nil, conv)
		}
	}

	iface, isIface := constraint.Underlying().(*types.Interface)
	if !isIface {
		return "", false
	}

	// Type-set unions (e.g. `~int | ~string`) also report IsComparable, so
	// the no-embeddeds check is essential to isolate the bare `comparable`.
	if iface.IsComparable() && iface.NumEmbeddeds() == 0 && iface.NumMethods() == 0 {
		return "Comparable", true
	}

	// Comparable type-set unions with no methods (e.g. math/rand/v2.intType =
	// `~int | ~int8 | ...`). Loses term precision; Go enforces the original
	// constraint on the generated call.
	if iface.NumMethods() == 0 && iface.NumEmbeddeds() > 0 && iface.IsComparable() {
		if allOrderedBasicTerms(iface) {
			return "Ordered", true
		}
		return "Comparable", true
	}

	// Method-set interfaces render by name, so embedding (e.g. hash.Hash embeds
	// io.Writer) is fine. Type-set unions are not method sets and cannot be a bound.
	if iface.IsMethodSet() && iface.NumMethods() > 0 {
		switch t := constraint.(type) {
		case *types.Named:
			return qualifyTypeNameBound(t.Obj(), t.TypeArgs(), conv)
		case *types.Alias:
			if isGenericAlias(t) {
				return "", false
			}
			return qualifyTypeNameBound(t.Obj(), t.TypeArgs(), conv)
		}
	}

	return "", false
}

func allOrderedBasicTerms(iface *types.Interface) bool {
	for etyp := range iface.EmbeddedTypes() {
		union, ok := etyp.(*types.Union)
		if !ok {
			return false
		}
		for term := range union.Terms() {
			basic, ok := term.Type().Underlying().(*types.Basic)
			if !ok {
				return false
			}
			if !isOrderedBasicKind(basic.Kind()) {
				return false
			}
		}
	}
	return true
}

func isOrderedBasicKind(kind types.BasicKind) bool {
	switch kind {
	case types.Int, types.Int8, types.Int16, types.Int32, types.Int64,
		types.Uint, types.Uint8, types.Uint16, types.Uint32, types.Uint64, types.Uintptr,
		types.Float32, types.Float64,
		types.String:
		return true
	}
	return false
}

// Renders a Named or Alias bound by its TypeName, qualifying with the package
// alias when external and tracking the external package on conv. Bounds in the
// current package render unqualified to avoid a self-import.
func qualifyTypeNameBound(obj *types.TypeName, typeArgs *types.TypeList, conv *Converter) (string, bool) {
	pkg := obj.Pkg()
	if pkg == nil {
		if obj.Name() == "error" {
			return "error", true
		}
		return "", false
	}
	if extract.IsInternalPackagePath(pkg.Path()) {
		return "", false
	}
	name := obj.Name()
	if conv == nil || pkg.Path() != conv.currentPkgPath {
		if conv != nil {
			conv.trackExternalPkg(pkg.Path(), pkg.Name())
		}
		name = PkgRef(pkg.Path()) + "." + obj.Name()
	}
	if typeArgs == nil || typeArgs.Len() == 0 {
		return name, true
	}
	args := make([]string, 0, typeArgs.Len())
	for arg := range typeArgs.Types() {
		result := ToLisette(arg, conv)
		if result.SkipReason != nil {
			return "", false
		}
		args = append(args, result.LisetteType)
	}
	return name + "<" + strings.Join(args, ", ") + ">", true
}

// Unwraps `interface { ~T }` to its inner T, the shared shape of every
// type-set-as-shape recognizer below.
func singleTildeTerm(constraint types.Type) (types.Type, bool) {
	iface, isIface := constraint.Underlying().(*types.Interface)
	if !isIface {
		return nil, false
	}
	if iface.NumEmbeddeds() != 1 {
		return nil, false
	}
	union, isUnion := iface.EmbeddedType(0).(*types.Union)
	if !isUnion || union.Len() != 1 {
		return nil, false
	}
	term := union.Term(0)
	if !term.Tilde() {
		return nil, false
	}
	return term.Type(), true
}

// Composed Lisette type for a collapsible constraint (`S ~[]E`, `A ~[N]E`,
// `M ~map[K]V`), so callers can rewrite the type parameter to its shape.
func collapsedShape(constraint types.Type) (string, bool) {
	if elemName, ok := recognizeSliceShape(constraint); ok {
		return sliceOf(elemName), true
	}
	if elemName, length, ok := recognizeArrayShape(constraint); ok {
		return arrayOf(elemName, length), true
	}
	if keyName, valName, ok := recognizeMapShape(constraint); ok {
		return mapOf(keyName, valName), true
	}
	return "", false
}

// Detects `S ~[]E` over a *types.TypeParam. Returns the inner E's name so
// callers can rewrite `S` to `Slice<E>`.
func recognizeSliceShape(constraint types.Type) (sliceElemTypeParamName string, ok bool) {
	inner, ok := singleTildeTerm(constraint)
	if !ok {
		return "", false
	}
	slice, isSlice := inner.(*types.Slice)
	if !isSlice {
		return "", false
	}
	tp, isTp := slice.Elem().(*types.TypeParam)
	if !isTp {
		return "", false
	}
	return tp.Obj().Name(), true
}

// Detects `A ~[N]E` over a *types.TypeParam element. Returns the inner E's name
// and the array length so callers can rewrite `A` to `Array<E, N>`.
func recognizeArrayShape(constraint types.Type) (elemName string, length int64, ok bool) {
	inner, ok := singleTildeTerm(constraint)
	if !ok {
		return "", 0, false
	}
	array, isArray := inner.(*types.Array)
	if !isArray {
		return "", 0, false
	}
	tp, isTp := array.Elem().(*types.TypeParam)
	if !isTp {
		return "", 0, false
	}
	return tp.Obj().Name(), array.Len(), true
}

// Detects `M ~map[K]V` over *types.TypeParam key and value. Returns the inner
// K's and V's names so callers can rewrite `M` to `Map<K, V>`.
func recognizeMapShape(constraint types.Type) (keyName, valName string, ok bool) {
	inner, ok := singleTildeTerm(constraint)
	if !ok {
		return "", "", false
	}
	m, isMap := inner.(*types.Map)
	if !isMap {
		return "", "", false
	}
	k, kIsTp := m.Key().(*types.TypeParam)
	v, vIsTp := m.Elem().(*types.TypeParam)
	if !kIsTp || !vIsTp {
		return "", "", false
	}
	return k.Obj().Name(), v.Obj().Name(), true
}
