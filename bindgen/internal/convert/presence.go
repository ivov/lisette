// SSA presence analysis: which trailing bools only report presence.
package convert

import (
	"go/constant"
	"go/token"
	"go/types"
	"slices"

	"golang.org/x/tools/go/ssa"
)

type presenceAnalysis struct {
	proofs     map[*ssa.Function]bool
	inProgress map[*ssa.Function]bool
}

func (a *NilnessAnalysis) recordPresence(fn *types.Func, presence *presenceAnalysis) {
	sig, ok := fn.Type().(*types.Signature)
	if !ok || sig.Results().Len() < 2 || !isBoolType(sig.Results().At(sig.Results().Len()-1).Type()) {
		return
	}
	if ssaFn := a.program.FuncValue(fn); ssaFn != nil && presence.function(ssaFn) {
		a.presence[fn] = true
	}
}

func (a *NilnessAnalysis) ReportsPresence(obj types.Object) bool {
	if a == nil {
		return false
	}
	fn, ok := obj.(*types.Func)
	return ok && a.presence[fn]
}

// function proves that every return with a false trailing bool gives zero values.
func (p *presenceAnalysis) function(fn *ssa.Function) bool {
	body := functionBody(fn)
	if body == nil || p.inProgress[body] {
		return false
	}
	if proven, ok := p.proofs[body]; ok {
		return proven
	}
	p.inProgress[body] = true
	proven := true
	for _, block := range body.Blocks {
		ret, ok := block.Instrs[len(block.Instrs)-1].(*ssa.Return)
		if !ok || len(ret.Results) < 2 {
			continue
		}
		last := len(ret.Results) - 1
		if !p.pair(ret.Results[:last], ret.Results[last], make(map[*ssa.Phi]bool)) {
			proven = false
			break
		}
	}
	delete(p.inProgress, body)
	p.proofs[body] = proven
	return proven
}

func (p *presenceAnalysis) pair(values []ssa.Value, flag ssa.Value, seen map[*ssa.Phi]bool) bool {
	if c, ok := flag.(*ssa.Const); ok && c.Value != nil && c.Value.Kind() == constant.Bool {
		present := constant.BoolVal(c.Value)
		for _, value := range values {
			if present && isNilConstant(value) && isDemotableGoType(value.Type()) {
				return false // Some cannot hold a nil reference
			}
			if !present && !isZeroValue(value) {
				return false
			}
		}
		return true
	}
	if phi, ok := flag.(*ssa.Phi); ok {
		if seen[phi] {
			return false
		}
		seen[phi] = true
		for i, edge := range phi.Edges {
			edgeValues := make([]ssa.Value, len(values))
			for j, value := range values {
				edgeValues[j] = value
				if valuePhi, ok := value.(*ssa.Phi); ok && valuePhi.Block() == phi.Block() {
					edgeValues[j] = valuePhi.Edges[i]
				}
			}
			if !p.pair(edgeValues, edge, seen) {
				return false
			}
		}
		return true
	}
	return p.commaOk(values, flag)
}

func (p *presenceAnalysis) commaOk(values []ssa.Value, flag ssa.Value) bool {
	flagExtract, ok := flag.(*ssa.Extract)
	if !ok || flagExtract.Index != len(values) {
		return false
	}
	if tuple, ok := flagExtract.Tuple.Type().(*types.Tuple); !ok || tuple.Len() != len(values)+1 {
		return false
	}
	for i, value := range values {
		extract, ok := value.(*ssa.Extract)
		if !ok || extract.Tuple != flagExtract.Tuple || extract.Index != i {
			return false
		}
	}
	switch producer := flagExtract.Tuple.(type) {
	case *ssa.Lookup:
		return producer.CommaOk
	case *ssa.TypeAssert:
		return producer.CommaOk
	case *ssa.UnOp:
		return producer.CommaOk
	case *ssa.Call:
		if callee := producer.Call.StaticCallee(); callee != nil {
			return p.function(callee)
		}
	}
	return false
}

func isZeroValue(value ssa.Value) bool {
	switch typed := value.(type) {
	case *ssa.Const:
		return typed.IsNil() || typed.Value == nil || isZeroConstant(typed.Value)
	case *ssa.UnOp:
		alloc, ok := typed.X.(*ssa.Alloc)
		if !ok || typed.Op != token.MUL {
			return false
		}
		for _, referrer := range *alloc.Referrers() {
			switch use := referrer.(type) {
			case *ssa.UnOp:
				if use.Op != token.MUL {
					return false
				}
			case *ssa.DebugRef:
			default:
				return false
			}
		}
		return true
	}
	return false
}

func isFalseConstant(value ssa.Value) bool {
	c, ok := value.(*ssa.Const)
	return ok && c.Value != nil && c.Value.Kind() == constant.Bool && !constant.BoolVal(c.Value)
}

func isNilConstant(value ssa.Value) bool {
	c, ok := value.(*ssa.Const)
	return ok && c.IsNil()
}

func isZeroConstant(value constant.Value) bool {
	switch value.Kind() {
	case constant.Bool:
		return !constant.BoolVal(value)
	case constant.String:
		return constant.StringVal(value) == ""
	case constant.Int, constant.Float, constant.Complex:
		return constant.Sign(value) == 0
	}
	return false
}

func (a *NilnessAnalysis) presentWhenTrue(values []ssa.Value, flag ssa.Value, stack, axioms []nilnessFact, seen map[*ssa.Phi]bool) bool {
	if isFalseConstant(flag) {
		return true
	}
	if phi, ok := flag.(*ssa.Phi); ok {
		if seen[phi] {
			return false
		}
		seen[phi] = true
		for i, edge := range phi.Edges {
			edgeStack := stack
			if fact, ok := edgeFact(phi.Block().Preds[i], phi.Block()); ok {
				edgeStack = append(slices.Clip(stack), fact)
			}
			edgeValues := make([]ssa.Value, len(values))
			for j, value := range values {
				edgeValues[j] = value
				if valuePhi, ok := value.(*ssa.Phi); ok && valuePhi.Block() == phi.Block() {
					edgeValues[j] = valuePhi.Edges[i]
				}
			}
			if !a.presentWhenTrue(edgeValues, edge, edgeStack, axioms, seen) {
				return false
			}
		}
		return true
	}
	for _, value := range values {
		if isDemotableGoType(value.Type()) && !a.present(value, stack, axioms) {
			return false
		}
	}
	return true
}

// present: emit reads an interface that holds a nil pointer as None.
func (a *NilnessAnalysis) present(value ssa.Value, stack, axioms []nilnessFact) bool {
	value = resolveSpill(value)
	if !types.IsInterface(value.Type()) {
		return a.eval(value, stack, make(map[ssa.Value]bool)).n == nilnessNonNil
	}
	switch typed := value.(type) {
	case *ssa.MakeInterface:
		switch typed.X.Type().Underlying().(type) {
		case *types.Pointer, *types.Map, *types.Chan, *types.Signature:
			return a.eval(typed.X, stack, make(map[ssa.Value]bool)).n == nilnessNonNil
		}
		return true
	case *ssa.Parameter:
		return slices.ContainsFunc(axioms, func(fact nilnessFact) bool {
			return fact.value == typed && fact.n == nilnessNonNil
		})
	}
	return false
}
