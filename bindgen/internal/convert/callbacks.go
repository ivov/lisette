// A call through a function value the analysis cannot name writes through its pointer-like arguments.
package convert

import (
	"go/token"
	"go/types"

	"golang.org/x/tools/go/ssa"
)

type callbackKey struct {
	parameter int
	position  int
}

type callbackFact struct {
	params   map[int]reachMode
	freeVars map[int]reachMode
}

func (m *mutationSummary) addCallback(key callbackKey, roots *mutationSummary) {
	if len(roots.params) == 0 && len(roots.freeVars) == 0 {
		return
	}
	if m.callbacks == nil {
		m.callbacks = map[callbackKey]*callbackFact{}
	}
	fact, ok := m.callbacks[key]
	if !ok {
		fact = &callbackFact{params: map[int]reachMode{}, freeVars: map[int]reachMode{}}
		m.callbacks[key] = fact
	}
	mergeModes(fact.params, roots.params)
	mergeModes(fact.freeVars, roots.freeVars)
}

func (m *mutationSummary) callbackWeight() int {
	total := 0
	for _, fact := range m.callbacks {
		total++
		for _, mode := range fact.params {
			total += int(mode)
		}
		for _, mode := range fact.freeVars {
			total += int(mode)
		}
	}
	return total
}

func (m *mutationSummary) externalCallbackRoots() map[int]reachMode {
	roots := map[int]reachMode{}
	for _, fact := range m.callbacks {
		mergeModes(roots, fact.params)
	}
	return roots
}

func mergeModes(into, from map[int]reachMode) {
	for index, mode := range from {
		into[index] |= mode
	}
}

type calleeSet struct {
	known      []*ssa.Function
	parameters []int
	unknown    bool
}

func (a *MutationAnalysis) resolveCallee(fn *ssa.Function, value ssa.Value, seen map[ssa.Value]bool) calleeSet {
	var set calleeSet
	if value == nil || seen[value] {
		return set
	}
	seen[value] = true
	switch typed := value.(type) {
	case *ssa.Function:
		set.known = append(set.known, typed)
	case *ssa.MakeClosure:
		if target, ok := typed.Fn.(*ssa.Function); ok {
			set.known = append(set.known, target)
		} else {
			set.unknown = true
		}
	case *ssa.Const:
	case *ssa.Parameter:
		if index := parameterIndex(fn, typed); index >= 0 {
			set.parameters = append(set.parameters, index)
		} else {
			set.unknown = true
		}
	case *ssa.Phi:
		for _, edge := range typed.Edges {
			set.merge(a.resolveCallee(fn, edge, seen))
		}
	case *ssa.ChangeType:
		set.merge(a.resolveCallee(fn, typed.X, seen))
	default:
		set.unknown = true
	}
	return set
}

func (s *calleeSet) merge(other calleeSet) {
	s.known = append(s.known, other.known...)
	s.parameters = append(s.parameters, other.parameters...)
	s.unknown = s.unknown || other.unknown
}

func (a *MutationAnalysis) recordCallbackCall(fn *ssa.Function, common *ssa.CallCommon, summary *mutationSummary) {
	set := a.resolveCallee(fn, common.Value, map[ssa.Value]bool{})
	for _, target := range set.known {
		a.applyKnownCallee(fn, target, common.Args, summary)
	}
	for position, argument := range common.Args {
		if !goWritableCapability(argument.Type()) {
			continue
		}
		roots := &mutationSummary{params: map[int]reachMode{}, freeVars: map[int]reachMode{}}
		a.argumentRoots(fn, argument, reachDirect, roots)
		if set.unknown {
			mergeModes(summary.params, roots.params)
			mergeModes(summary.freeVars, roots.freeVars)
		}
		for _, parameter := range set.parameters {
			if yieldDeclaredReadOnly(fn, parameter, position) {
				continue
			}
			summary.addCallback(callbackKey{parameter, position}, roots)
		}
	}
}

func (a *MutationAnalysis) applyKnownCallee(fn *ssa.Function, target *ssa.Function, args []ssa.Value, summary *mutationSummary) {
	for index, mode := range a.knownWrites(target) {
		if index < len(args) {
			a.argumentRoots(fn, args[index], mode, summary)
		}
	}
}

// knownWrites includes the unresolved callbacks of target, since a function value hides who supplies them.
func (a *MutationAnalysis) knownWrites(target *ssa.Function) map[int]reachMode {
	written := map[int]reachMode{}
	if functionBody(target) == nil {
		return written
	}
	inner := a.summarize(target)
	mergeModes(written, inner.params)
	mergeModes(written, inner.externalCallbackRoots())
	return written
}

func (a *MutationAnalysis) argumentRoots(fn *ssa.Function, argument ssa.Value, mode reachMode, into *mutationSummary) {
	resumeWalk(fn, argument, into, mode)
	base := argument
	if load, ok := base.(*ssa.UnOp); ok && load.Op == token.MUL {
		base = load.X
	}
	a.packedRoots(fn, base, into, map[ssa.Value]bool{})
}

func (a *MutationAnalysis) packedRoots(fn *ssa.Function, value ssa.Value, into *mutationSummary, seen map[ssa.Value]bool) {
	root, _ := storagePath(value)
	if seen[root] {
		return
	}
	seen[root] = true
	switch root.(type) {
	case *ssa.Alloc, *ssa.MakeSlice, *ssa.MakeMap:
	default:
		return
	}
	values, _, _ := a.containedValues(root)
	for _, stored := range values {
		if !goWritableCapability(stored.value.Type()) {
			continue
		}
		resumeWalk(fn, stored.value, into, reachDirect)
		a.packedRoots(fn, stored.value, into, seen)
	}
}

func (a *MutationAnalysis) resolveCalleeCallbacks(fn *ssa.Function, common *ssa.CallCommon, inner *mutationSummary, summary *mutationSummary) {
	closure, _ := common.Value.(*ssa.MakeClosure)
	for key, fact := range inner.callbacks {
		roots := &mutationSummary{params: map[int]reachMode{}, freeVars: map[int]reachMode{}}
		for index, mode := range fact.params {
			if index < len(common.Args) {
				a.argumentRoots(fn, common.Args[index], mode, roots)
			}
		}
		for index, mode := range fact.freeVars {
			if closure != nil && index < len(closure.Bindings) {
				resumeWalk(fn, closure.Bindings[index], roots, mode)
			}
		}
		if len(roots.params) == 0 && len(roots.freeVars) == 0 {
			continue
		}
		set := calleeSet{unknown: true}
		if key.parameter < len(common.Args) {
			set = a.resolveCallee(fn, common.Args[key.parameter], map[ssa.Value]bool{})
		}
		if set.unknown {
			mergeModes(summary.params, roots.params)
			mergeModes(summary.freeVars, roots.freeVars)
		}
		for _, target := range set.known {
			if mode, wrote := a.knownWrites(target)[key.position]; wrote {
				mergeModes(summary.params, composeModes(roots.params, mode))
				mergeModes(summary.freeVars, composeModes(roots.freeVars, mode))
			}
		}
		for _, parameter := range set.parameters {
			summary.addCallback(callbackKey{parameter, key.position}, roots)
		}
	}
}

func composeModes(roots map[int]reachMode, callee reachMode) map[int]reachMode {
	out := map[int]reachMode{}
	for index, mode := range roots {
		if mode&reachThroughLoad != 0 {
			out[index] = reachThroughLoad
		} else {
			out[index] = callee
		}
	}
	return out
}

func (a *MutationAnalysis) resolveClosureCallbacks(fn *ssa.Function, closure *ssa.MakeClosure, inner *mutationSummary, summary *mutationSummary) {
	for _, fact := range inner.callbacks {
		for index, mode := range fact.freeVars {
			if index < len(closure.Bindings) {
				resumeWalk(fn, closure.Bindings[index], summary, mode)
			}
		}
		mergeModes(inner.params, fact.params)
	}
}

// yieldDeclaredReadOnly: Lisette types the yield argument of a returned iter.Seq[V] instance by the type parameter, read-only.
func yieldDeclaredReadOnly(fn *ssa.Function, parameter, position int) bool {
	parent := fn.Parent()
	if parent == nil {
		return false
	}
	returned := map[int]bool{}
	made := false
	for _, block := range parent.Blocks {
		for _, instruction := range block.Instrs {
			closure, ok := instruction.(*ssa.MakeClosure)
			if !ok || closure.Fn != fn {
				continue
			}
			made = true
			if !returnPositions(closure, returned, map[ssa.Value]bool{}) {
				return false
			}
		}
	}
	if !made || len(returned) == 0 {
		return false
	}
	results := parent.Signature.Results()
	for index := range returned {
		if !declaredPositionReadOnly(results.At(index).Type(), parameter, position) {
			return false
		}
	}
	return true
}

func returnPositions(value ssa.Value, out map[int]bool, seen map[ssa.Value]bool) bool {
	if seen[value] {
		return true
	}
	seen[value] = true
	referrers := value.Referrers()
	if referrers == nil {
		return true
	}
	for _, referrer := range *referrers {
		switch typed := referrer.(type) {
		case *ssa.Return:
			for index, result := range typed.Results {
				if result == value {
					out[index] = true
				}
			}
		case *ssa.ChangeType:
			if !returnPositions(typed, out, seen) {
				return false
			}
		case *ssa.Phi:
			if !returnPositions(typed, out, seen) {
				return false
			}
		case *ssa.DebugRef:
		default:
			return false
		}
	}
	return true
}

func declaredPositionReadOnly(t types.Type, parameter, position int) bool {
	origin := instantiatedOriginSignature(t)
	if origin == nil || parameter >= origin.Params().Len() {
		return false
	}
	callback, ok := types.Unalias(origin.Params().At(parameter).Type()).(*types.Signature)
	if !ok || position >= callback.Params().Len() {
		return false
	}
	_, isTypeParam := callback.Params().At(position).Type().(*types.TypeParam)
	return isTypeParam
}

func instantiatedOriginSignature(t types.Type) *types.Signature {
	named, ok := types.Unalias(t).(*types.Named)
	if !ok || named.TypeArgs().Len() == 0 {
		return nil
	}
	signature, _ := named.Origin().Underlying().(*types.Signature)
	return signature
}
