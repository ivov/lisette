package convert

import (
	"go/types"
	"strings"
)

type TypeParamSpec struct {
	Name  string
	Bound string
}

type TypeParamSpecs []TypeParamSpec

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
// as plain `comparable`. When the constraint lives in currentPkgPath, the
// bound is rendered unqualified to avoid a self-import.
func recognizeBound(constraint types.Type, currentPkgPath string) (boundExpr string, ok bool, requiresImports []string) {
	if named, isNamed := constraint.(*types.Named); isNamed {
		obj := named.Obj()
		if obj.Pkg() != nil && obj.Pkg().Path() == "cmp" && obj.Name() == "Ordered" {
			if currentPkgPath == "cmp" {
				return "Ordered", true, nil
			}
			return "cmp.Ordered", true, []string{"cmp"}
		}
	}

	iface, isIface := constraint.Underlying().(*types.Interface)
	if !isIface {
		return "", false, nil
	}

	// Type-set unions (e.g. `~int | ~string`) also report IsComparable, so
	// the no-embeddeds check is essential to isolate the bare `comparable`.
	if iface.IsComparable() && iface.NumEmbeddeds() == 0 && iface.NumMethods() == 0 {
		return "Comparable", true, nil
	}

	return "", false, nil
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
