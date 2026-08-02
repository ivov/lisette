// Interprocedural summaries resolved at call sites.
package convert

import (
	"go/types"

	"golang.org/x/tools/go/ssa"
)

// nilnessSummary is one function's digest, computed without boundary axioms.
type nilnessSummary struct {
	results []nilnessValue
	// delegates[i]: parameter indices flowing directly to result i
	delegates []map[int]bool
	// nilSites[i]: per nil site, the parameter-nil guards that dominated
	// it; a non-nil argument discharges, an empty list never does
	nilSites [][][]int
	// exclusive: every site has results[0] or results[last] non-nil
	exclusive bool
	// nilWithNilError[i]: some site returns result i nil with a nil error
	nilWithNilError []bool
}

const nilnessMaxDepth = 32

// summarize memoizes; a function on the summarization stack yields the
// zero summary.
func (a *NilnessAnalysis) summarize(fn *ssa.Function) *nilnessSummary {
	if s, ok := a.summaries[fn]; ok {
		return s
	}
	if a.summaryDepth > nilnessMaxDepth {
		return &nilnessSummary{} // not memoized: shallow by depth, not by content
	}
	a.summaryDepth++
	defer func() { a.summaryDepth-- }()
	s := &nilnessSummary{}
	a.summaries[fn] = s

	body := functionBody(fn)
	if body == nil {
		return s
	}

	resultCount := body.Signature.Results().Len()
	s.results = make([]nilnessValue, resultCount)
	s.delegates = make([]map[int]bool, resultCount)
	s.nilSites = make([][][]int, resultCount)
	s.nilWithNilError = make([]bool, resultCount)
	s.exclusive = resultCount >= 2
	lastIsError := resultCount >= 2 && isErrorType(body.Signature.Results().At(resultCount-1).Type())
	contributed := make([]bool, resultCount)

	sawReturn := a.walkReturnSites(body, nil, func(ret *ssa.Return, dominating []nilnessFact, values []nilnessValue) {
		if lastIsError {
			for i, forwarded := range a.forwardedNilNil(ret, values) {
				if forwarded {
					s.nilWithNilError[i] = true
				}
			}
		}
		for i, v := range ret.Results {
			if values[i].witness {
				var guards []int
				for _, f := range dominating {
					if f.n == nilnessNil {
						if j := parameterIndex(body, f.value); j >= 0 {
							guards = append(guards, j)
						}
					}
				}
				s.nilSites[i] = append(s.nilSites[i], guards)
				if values[i].n == nilnessNil && lastIsError && values[resultCount-1].n == nilnessNil {
					s.nilWithNilError[i] = true
				}
				// nil under parameter-nil facts: delegate, not a hard nil
				if values[i].n == nilnessNil && len(guards) > 0 {
					if s.delegates[i] == nil {
						s.delegates[i] = make(map[int]bool)
					}
					for _, j := range guards {
						s.delegates[i][j] = true
					}
					continue
				}
			}
			if values[i].n != nilnessNonNil {
				if delegated := a.delegateParams(body, v, dominating, 0); delegated != nil {
					if s.delegates[i] == nil {
						s.delegates[i] = make(map[int]bool)
					}
					for _, j := range delegated {
						s.delegates[i][j] = true
					}
					continue // site contributes via delegation, not the meet
				}
			}
			if contributed[i] {
				s.results[i] = meetNilness(s.results[i], values[i])
			} else {
				s.results[i] = values[i]
			}
			contributed[i] = true
		}
		if s.exclusive && !a.siteExclusive(ret, values) {
			s.exclusive = false
		}
	})
	for i := range s.results {
		switch {
		case !sawReturn: // no reachable return (infinite loop or always panics)
			s.results[i] = nilnessValue{}
		case !contributed[i] && s.delegates[i] != nil:
			s.results[i] = nilnessValue{n: nilnessNonNil} // identity for delegate meet
		}
	}
	if !sawReturn {
		s.exclusive = false
	}

	a.applyCalleePin(fn, s)
	return s
}

// applyCalleePin: a non_nilable_return pin stands in for a callee proof.
func (a *NilnessAnalysis) applyCalleePin(fn *ssa.Function, s *nilnessSummary) {
	if a.cfg == nil || len(s.results) != 1 {
		return
	}
	origin := fn
	if fn.Origin() != nil {
		origin = fn.Origin()
	}
	obj, ok := origin.Object().(*types.Func)
	if !ok || obj.Pkg() == nil {
		return
	}
	sig, ok := obj.Type().(*types.Signature)
	if !ok || !isSingleDemotableResult(sig) {
		return
	}
	if a.cfg.IsNonNilableReturn(obj.Pkg().Path(), qualifiedFunctionName(obj)) {
		s.results[0] = nilnessValue{n: nilnessNonNil}
		s.nilSites[0] = nil
		s.delegates[0] = nil
	}
}

// functionBody: bodiless instantiations fall back to the generic origin.
func functionBody(fn *ssa.Function) *ssa.Function {
	if fn.Blocks != nil {
		return fn
	}
	if origin := fn.Origin(); origin != nil && origin != fn && origin.Blocks != nil {
		return origin
	}
	return nil
}

func parameterIndex(fn *ssa.Function, v ssa.Value) int {
	for j, p := range fn.Params {
		if v == p {
			return j
		}
	}
	return -1
}

// callTarget resolves v to a call and result index, a bare call as index 0.
func callTarget(v ssa.Value) (*ssa.Call, int) {
	switch t := v.(type) {
	case *ssa.Call:
		return t, 0
	case *ssa.Extract:
		if call, ok := t.Tuple.(*ssa.Call); ok {
			return call, t.Index
		}
	}
	return nil, 0
}

func (a *NilnessAnalysis) projectCall(v ssa.Value) (*nilnessSummary, *ssa.Call, int, bool) {
	call, index := callTarget(v)
	if call == nil {
		return nil, nil, 0, false
	}
	callee := call.Common().StaticCallee()
	if callee == nil {
		return nil, nil, 0, false
	}
	return a.summarize(callee), call, index, true
}

// delegateParams reduces v's nilness to body parameters through delegating
// callees. nil = no reduction, empty = non-nil outright.
func (a *NilnessAnalysis) delegateParams(body *ssa.Function, v ssa.Value, dominating []nilnessFact, depth int) []int {
	if j := parameterIndex(body, v); j >= 0 {
		return []int{j}
	}
	if depth > 4 {
		return nil
	}
	if spilled := resolveSpill(v); spilled != v {
		return a.delegateParams(body, spilled, dominating, depth)
	}
	s, call, index, ok := a.projectCall(v)
	if !ok {
		return nil
	}
	if index >= len(s.results) || s.results[index].n != nilnessNonNil || len(s.nilSites[index]) > 0 {
		return nil
	}
	out := []int{}
	for j := range s.delegates[index] {
		if j >= len(call.Common().Args) {
			return nil
		}
		arg := call.Common().Args[j]
		if k := parameterIndex(body, arg); k >= 0 {
			out = append(out, k)
			continue
		}
		if a.eval(arg, dominating, make(map[ssa.Value]bool)).n == nilnessNonNil {
			continue
		}
		if sub := a.delegateParams(body, arg, dominating, depth+1); sub != nil {
			out = append(out, sub...)
			continue
		}
		return nil
	}
	return out
}

func (a *NilnessAnalysis) callResult(s *nilnessSummary, index int, args []ssa.Value, dominating []nilnessFact, visiting map[ssa.Value]bool) nilnessValue {
	r := s.results[index]
	r.witness = false
	for _, guards := range s.nilSites[index] {
		discharged := false
		for _, j := range guards {
			if j < len(args) && a.eval(args[j], dominating, visiting).n == nilnessNonNil {
				discharged = true
				break
			}
		}
		if !discharged {
			r.witness = true
			break
		}
	}
	for j := range s.delegates[index] {
		if j < len(args) {
			r = meetNilness(r, a.eval(args[j], dominating, visiting))
		} else {
			r = meetNilness(r, nilnessValue{})
		}
	}
	return r
}

func (a *NilnessAnalysis) siteExclusive(ret *ssa.Return, values []nilnessValue) bool {
	last := len(ret.Results) - 1
	if values[0].n == nilnessNonNil || values[last].n == nilnessNonNil {
		return true
	}
	s, call, index, ok := a.projectCall(ret.Results[0])
	if !ok || index != 0 || !s.exclusive || len(s.results) != len(ret.Results) {
		return false
	}
	lastExtract, ok := ret.Results[last].(*ssa.Extract)
	return ok && lastExtract.Tuple == call && lastExtract.Index == last
}
