// Recover-block returns, modeled conservatively.
package convert

import (
	"go/types"

	"golang.org/x/tools/go/ssa"
)

// siteValues admits Recover-block returns only under a provable recover.
func (a *NilnessAnalysis) siteValues(body *ssa.Function, ret *ssa.Return, dominating []nilnessFact) ([]nilnessValue, bool) {
	values := make([]nilnessValue, len(ret.Results))
	if body.Recover != nil && ret.Block() == body.Recover {
		if !a.staticallyRecovers(body) {
			return nil, false
		}
		// A recovered panic may leave any nilable result at its zero value
		// (a zero error is nil too). Restoring handlers are pinned in config.
		results := body.Signature.Results()
		for i := range ret.Results {
			switch results.At(i).Type().Underlying().(type) {
			case *types.Pointer, *types.Interface, *types.Signature, *types.Map, *types.Slice, *types.Chan:
				values[i] = nilnessValue{n: nilnessNil, witness: true}
			}
		}
		return values, true
	}
	for i, v := range ret.Results {
		values[i] = a.eval(v, dominating, make(map[ssa.Value]bool))
	}
	return values, true
}

// staticallyRecovers: accepted unsoundness, a dynamic deferred callback
// that calls recover is invisible here.
func (a *NilnessAnalysis) staticallyRecovers(fn *ssa.Function) bool {
	if answer, ok := a.staticRecovers[fn]; ok {
		return answer
	}
	answer := false
	for _, block := range fn.Blocks {
		for _, instr := range block.Instrs {
			deferred, ok := instr.(*ssa.Defer)
			if !ok {
				continue
			}
			if callee := deferred.Common().StaticCallee(); callee != nil && callsRecover(callee) {
				answer = true
				break
			}
		}
		if answer {
			break
		}
	}
	a.staticRecovers[fn] = answer
	return answer
}

func callsRecover(fn *ssa.Function) bool {
	for _, block := range fn.Blocks {
		for _, instr := range block.Instrs {
			if call, ok := instr.(*ssa.Call); ok {
				if builtin, ok := call.Common().Value.(*ssa.Builtin); ok && builtin.Name() == "recover" {
					return true
				}
			}
		}
	}
	return false
}
