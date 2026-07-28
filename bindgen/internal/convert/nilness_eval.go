// Value evaluation: the nilness lattice, dominator walk, and edge facts.
package convert

import (
	"go/token"
	"go/types"
	"strings"

	"golang.org/x/tools/go/ssa"
)

type nilness int8

const (
	nilnessNonNil  nilness = -1
	nilnessUnknown nilness = 0
	nilnessNil     nilness = 1
)

// witness: a concrete nil-producing path exists, not mere ignorance.
type nilnessValue struct {
	n       nilness
	witness bool
}

func meetNilness(a, b nilnessValue) nilnessValue {
	n := a.n
	if a.n != b.n {
		n = nilnessUnknown
	}
	return nilnessValue{n, a.witness || b.witness}
}

type nilnessFact struct {
	value ssa.Value
	n     nilness
}

func nilComparison(b *ssa.BasicBlock) (op *ssa.BinOp, trueSucc, falseSucc *ssa.BasicBlock) {
	branch, ok := b.Instrs[len(b.Instrs)-1].(*ssa.If)
	if !ok {
		return nil, nil, nil
	}
	binop, ok := branch.Cond.(*ssa.BinOp)
	if !ok {
		return nil, nil, nil
	}
	switch binop.Op {
	case token.EQL:
		return binop, b.Succs[0], b.Succs[1]
	case token.NEQ:
		return binop, b.Succs[1], b.Succs[0]
	}
	return nil, nil, nil
}

// walk mirrors the x/tools nilness pass: dominator order, edge facts.
func (a *NilnessAnalysis) walk(fn *ssa.Function, axioms []nilnessFact, found func(*ssa.Return, []nilnessFact)) {
	if fn.Blocks == nil {
		return
	}
	seen := make([]bool, len(fn.Blocks))
	var visit func(b *ssa.BasicBlock, stack []nilnessFact)
	visit = func(b *ssa.BasicBlock, stack []nilnessFact) {
		if seen[b.Index] {
			return
		}
		seen[b.Index] = true

		for _, instr := range b.Instrs {
			if ret, ok := instr.(*ssa.Return); ok {
				found(ret, stack)
			}
		}

		if binop, trueSucc, falseSucc := nilComparison(b); binop != nil {
			xn := a.eval(binop.X, stack, make(map[ssa.Value]bool))
			yn := a.eval(binop.Y, stack, make(map[ssa.Value]bool))

			if xn.n != nilnessUnknown && yn.n != nilnessUnknown && (xn.n == nilnessNil || yn.n == nilnessNil) {
				var skip *ssa.BasicBlock
				if xn.n == yn.n {
					skip = falseSucc
				} else {
					skip = trueSucc
				}
				for _, d := range b.Dominees() {
					if d == skip && len(d.Preds) == 1 {
						continue
					}
					visit(d, stack)
				}
				return
			}

			if xn.n == nilnessNil || yn.n == nilnessNil {
				var v ssa.Value
				if xn.n == nilnessNil {
					v = binop.Y
				} else {
					v = binop.X
				}
				for _, d := range b.Dominees() {
					s := stack
					if len(d.Preds) == 1 {
						// append-share is safe: siblings only read their
						// own logical length
						switch d {
						case trueSucc:
							s = append(stack, nilnessFact{v, nilnessNil})
						case falseSucc:
							s = append(stack, nilnessFact{v, nilnessNonNil})
						}
					}
					visit(d, s)
				}
				return
			}
		}

		for _, d := range b.Dominees() {
			visit(d, stack)
		}
	}
	visit(fn.Blocks[0], axioms)
	// the Recover block is a second entry point the dominator walk misses
	if fn.Recover != nil && !seen[fn.Recover.Index] {
		visit(fn.Recover, axioms)
	}
}

func edgeFact(pred, succ *ssa.BasicBlock) (nilnessFact, bool) {
	binop, trueSucc, falseSucc := nilComparison(pred)
	if binop == nil {
		return nilnessFact{}, false
	}
	var v ssa.Value
	if isNilConst(binop.X) {
		v = binop.Y
	} else if isNilConst(binop.Y) {
		v = binop.X
	} else {
		return nilnessFact{}, false
	}
	if succ == trueSucc {
		return nilnessFact{v, nilnessNil}, true
	}
	if succ == falseSucc {
		return nilnessFact{v, nilnessNonNil}, true
	}
	return nilnessFact{}, false
}

func (a *NilnessAnalysis) eval(v ssa.Value, stack []nilnessFact, visiting map[ssa.Value]bool) nilnessValue {
	if visiting[v] {
		return nilnessValue{}
	}
	visiting[v] = true
	defer delete(visiting, v)

	for i := len(stack) - 1; i >= 0; i-- {
		if stack[i].value == v {
			return nilnessValue{stack[i].n, stack[i].n == nilnessNil}
		}
	}

	switch v := v.(type) {
	case *ssa.Alloc, *ssa.FieldAddr, *ssa.Function,
		*ssa.Global, *ssa.IndexAddr, *ssa.MakeChan, *ssa.MakeClosure,
		*ssa.MakeMap, *ssa.MakeSlice:
		return nilnessValue{n: nilnessNonNil}

	case *ssa.FreeVar:
		// a bound-method wrapper's free variable is the receiver, maybe nil
		if parent := v.Parent(); parent != nil && strings.HasPrefix(parent.Synthetic, "bound method wrapper") {
			return nilnessValue{}
		}
		return nilnessValue{n: nilnessNonNil}

	case *ssa.Const:
		if v.IsNil() {
			return nilnessValue{nilnessNil, true}
		}
		return nilnessValue{}

	case *ssa.MakeInterface:
		if _, isTypeParam := types.Unalias(v.X.Type()).(*types.TypeParam); isTypeParam {
			return nilnessValue{}
		}
		return nilnessValue{n: nilnessNonNil}

	case *ssa.ChangeInterface:
		return a.eval(v.X, stack, visiting)

	case *ssa.ChangeType:
		return a.eval(v.X, stack, visiting)

	case *ssa.Slice:
		return a.eval(v.X, stack, visiting)

	case *ssa.TypeAssert:
		// non-comma-ok assert to an interface: success implies non-nil
		if !v.CommaOk {
			if _, ok := v.AssertedType.Underlying().(*types.Interface); ok {
				return nilnessValue{n: nilnessNonNil}
			}
		}
		return nilnessValue{}

	case *ssa.Phi:
		// visiting grows once per eval recursion, bounding Phi nesting
		if len(visiting) > nilnessMaxDepth {
			return nilnessValue{}
		}
		result := nilnessValue{n: nilnessNonNil} // identity for meet
		for i, edge := range v.Edges {
			pred := v.Block().Preds[i]
			s := stack
			if f, ok := edgeFact(pred, v.Block()); ok {
				s = append(stack, f)
			}
			result = meetNilness(result, a.eval(edge, s, visiting))
		}
		return result

	case *ssa.Call:
		s, call, _, ok := a.projectCall(v)
		if !ok || len(s.results) != 1 {
			return nilnessValue{}
		}
		return a.callResult(s, 0, call.Common().Args, stack, visiting)

	case *ssa.Extract:
		s, call, index, ok := a.projectCall(v)
		if !ok || index >= len(s.results) {
			return nilnessValue{}
		}
		r := a.callResult(s, index, call.Common().Args, stack, visiting)
		if r.n != nilnessUnknown {
			return r
		}
		// exclusive callee + dominating err == nil proves result 0 non-nil
		if v.Index == 0 && s.exclusive {
			for _, ref := range *call.Referrers() {
				if sibling, ok := ref.(*ssa.Extract); ok && sibling.Index == len(s.results)-1 {
					if a.eval(sibling, stack, visiting).n == nilnessNil {
						return nilnessValue{n: nilnessNonNil}
					}
				}
			}
		}
		return r

	case *ssa.UnOp:
		if v.Op == token.MUL {
			if g, ok := v.X.(*ssa.Global); ok {
				return nilnessValue{n: a.globalNilness(g)}
			}
		}
		if spilled := resolveSpill(v); spilled != v {
			return a.eval(spilled, stack, visiting)
		}
		return nilnessValue{}
	}

	return nilnessValue{}
}

func allocIsPrivate(alloc *ssa.Alloc) bool {
	for _, ref := range *alloc.Referrers() {
		switch use := ref.(type) {
		case *ssa.Store:
			if use.Addr != alloc || use.Val == alloc {
				return false
			}
		case *ssa.UnOp:
			if use.Op != token.MUL {
				return false
			}
		default:
			return false
		}
	}
	return true
}

func isNilConst(v ssa.Value) bool {
	constant, ok := v.(*ssa.Const)
	return ok && constant.IsNil()
}

// resolveSpill sees through the return spill slot that defer lowering creates.
func resolveSpill(v ssa.Value) ssa.Value {
	load, ok := v.(*ssa.UnOp)
	if !ok || load.Op != token.MUL {
		return v
	}
	if alloc, ok := load.X.(*ssa.Alloc); ok && allocIsPrivate(alloc) {
		if stored := lastStoreBefore(alloc, load); stored != nil {
			return stored
		}
	}
	return v
}

// lastStoreBefore only looks within load's own block.
func lastStoreBefore(alloc *ssa.Alloc, load *ssa.UnOp) ssa.Value {
	var stored ssa.Value
	for _, instr := range load.Block().Instrs {
		if instr == load {
			return stored
		}
		if st, ok := instr.(*ssa.Store); ok && st.Addr == alloc {
			stored = st.Val
		}
	}
	return nil
}
