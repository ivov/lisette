// SSA return-nilability analysis: whole-program facts for conversion.
package convert

import (
	"fmt"
	"go/types"
	"os"
	"slices"
	"sort"

	"github.com/ivov/lisette/bindgen/internal/config"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
	"golang.org/x/tools/go/ssa/ssautil"
)

type ReturnNilability int8

const (
	ReturnUnknown ReturnNilability = iota
	ReturnProvenNonNil
	ReturnHasNilPath
)

type FunctionNilness struct {
	HasBody bool
	// Single applies when the sole result is nilable.
	Single ReturnNilability
	// NilWithNilError[i]: some site returns result i nil with a nil error.
	NilWithNilError []bool
	// NilWitness[i]: some site returns result i nil.
	NilWitness []bool
}

// Precomputed at construction, read-only afterwards.
type NilnessAnalysis struct {
	program        *ssa.Program
	cfg            *config.Config
	verdicts       map[*types.Func]FunctionNilness
	summaries      map[*ssa.Function]*nilnessSummary
	staticRecovers map[*ssa.Function]bool
	// summaryDepth bounds acyclic recursion; cycles hit the memo placeholder
	summaryDepth int

	globalUsages map[*ssa.Global]*globalUsage
	globalFacts  map[*ssa.Global]nilness
}

// NewNilnessAnalysis returns nil when SSA construction fails, leaving
// callers on heuristics only.
func NewNilnessAnalysis(roots []*packages.Package, cfg *config.Config) (analysis *NilnessAnalysis) {
	wellTyped := make([]*packages.Package, 0, len(roots))
	for _, pkg := range roots {
		if pkg != nil && pkg.Types != nil && len(pkg.Errors) == 0 {
			wellTyped = append(wellTyped, pkg)
		}
	}
	if len(wellTyped) == 0 {
		return nil
	}
	defer func() {
		if r := recover(); r != nil {
			fmt.Fprintf(os.Stderr, "bindgen: nilability analysis unavailable (SSA build failed: %v), falling back to heuristics\n", r)
			analysis = nil
		}
	}()
	program, _ := ssautil.AllPackages(wellTyped, ssa.InstantiateGenerics)
	program.Build()

	analysis = &NilnessAnalysis{
		program:        program,
		cfg:            cfg,
		verdicts:       make(map[*types.Func]FunctionNilness),
		summaries:      make(map[*ssa.Function]*nilnessSummary),
		staticRecovers: make(map[*ssa.Function]bool),
		globalFacts:    make(map[*ssa.Global]nilness),
	}

	sorted := make([]*packages.Package, len(wellTyped))
	copy(sorted, wellTyped)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].PkgPath < sorted[j].PkgPath })

	// cycle verdicts depend on summarization order, so fix it here
	for _, pkg := range sorted {
		if pkg.Types == nil {
			continue
		}
		scope := pkg.Types.Scope()
		for _, name := range scope.Names() {
			switch obj := scope.Lookup(name).(type) {
			case *types.Func:
				analysis.record(obj)
			case *types.TypeName:
				named, ok := obj.Type().(*types.Named)
				if !ok {
					continue
				}
				for sel := range program.MethodSets.MethodSet(types.NewPointer(named)).Methods() {
					if fn, ok := sel.Obj().(*types.Func); ok {
						analysis.record(fn)
					}
				}
			}
		}
	}

	return analysis
}

func (a *NilnessAnalysis) Function(obj types.Object) (FunctionNilness, bool) {
	if a == nil {
		return FunctionNilness{}, false
	}
	fn, ok := obj.(*types.Func)
	if !ok {
		return FunctionNilness{}, false
	}
	facts, ok := a.verdicts[fn]
	return facts, ok
}

func (a *NilnessAnalysis) record(fn *types.Func) {
	if _, done := a.verdicts[fn]; done {
		return
	}
	ssaFn := a.program.FuncValue(fn)
	if ssaFn == nil {
		a.verdicts[fn] = FunctionNilness{}
		return
	}
	var nilableParams []string
	if a.cfg != nil && fn.Pkg() != nil {
		nilableParams = a.cfg.NilableParams(fn.Pkg().Path(), qualifiedFunctionName(fn))
	}
	a.verdicts[fn] = a.analyze(ssaFn, nilableParams)
}

// qualifiedFunctionName matches the config key format.
func qualifiedFunctionName(fn *types.Func) string {
	sig, ok := fn.Type().(*types.Signature)
	if !ok || sig.Recv() == nil {
		return fn.Name()
	}
	recv := sig.Recv().Type()
	if ptr, ok := recv.(*types.Pointer); ok {
		recv = ptr.Elem()
	}
	if named, ok := recv.(*types.Named); ok {
		return named.Obj().Name() + "." + fn.Name()
	}
	return fn.Name()
}

func (a *NilnessAnalysis) analyze(fn *ssa.Function, nilableParams []string) FunctionNilness {
	body := functionBody(fn)
	if body == nil {
		return FunctionNilness{}
	}

	results := body.Signature.Results()
	anyNilable := false
	for v := range results.Variables() {
		if isNilableGoType(v.Type()) {
			anyNilable = true
			break
		}
	}
	if !anyNilable {
		return FunctionNilness{HasBody: true} // no consumer reads these facts
	}

	resultCount := results.Len()
	facts := FunctionNilness{
		HasBody:         true,
		NilWithNilError: make([]bool, resultCount),
		NilWitness:      make([]bool, resultCount),
	}
	lastIsError := resultCount >= 2 && isErrorType(results.At(resultCount-1).Type())

	merged := nilnessValue{n: nilnessNonNil}

	sawReturn := a.walkReturnSites(body, a.boundaryAxioms(body, nilableParams), func(ret *ssa.Return, dominating []nilnessFact, values []nilnessValue) {
		// NilWithNilError needs a provable (nil, nil): a bare inherited
		// witness is usually a lost error correlation.
		for i := range values {
			if values[i].witness {
				facts.NilWitness[i] = true
			}
			if values[i].n == nilnessNil && lastIsError && values[resultCount-1].n == nilnessNil {
				facts.NilWithNilError[i] = true
			}
		}
		if lastIsError {
			for i, forwarded := range a.forwardedNilNil(ret, values) {
				if forwarded {
					facts.NilWithNilError[i] = true
					facts.NilWitness[i] = true
				}
			}
		}
		if resultCount == 1 {
			merged = meetNilness(merged, values[0])
		}
	})

	if resultCount == 1 {
		switch {
		case merged.witness:
			facts.Single = ReturnHasNilPath
		case sawReturn && merged.n == nilnessNonNil:
			facts.Single = ReturnProvenNonNil
		}
	}
	return facts
}

// walkReturnSites owns site admission.
func (a *NilnessAnalysis) walkReturnSites(body *ssa.Function, axioms []nilnessFact, visit func(*ssa.Return, []nilnessFact, []nilnessValue)) bool {
	resultCount := body.Signature.Results().Len()
	sawReturn := false
	a.walk(body, axioms, func(ret *ssa.Return, dominating []nilnessFact) {
		if len(ret.Results) != resultCount {
			return
		}
		values, ok := a.siteValues(body, ret, dominating)
		if !ok {
			return
		}
		sawReturn = true
		visit(ret, dominating, values)
	})
	return sawReturn
}

// forwardedNilNil: which results inherit a callee (nil, nil) witness here.
func (a *NilnessAnalysis) forwardedNilNil(ret *ssa.Return, values []nilnessValue) []bool {
	last := len(ret.Results) - 1
	var out []bool
	for i := 0; i < last; i++ {
		if values[i].n == nilnessNonNil {
			continue // a dominating guard proved this value non-nil here
		}
		s, call, index, ok := a.projectCall(resolveSpill(ret.Results[i]))
		if !ok || index >= len(s.nilWithNilError) || !s.nilWithNilError[index] {
			continue
		}
		errNil := values[last].n == nilnessNil
		if !errNil {
			if errExtract, ok := resolveSpill(ret.Results[last]).(*ssa.Extract); ok {
				errNil = errExtract.Tuple == call && errExtract.Index == len(s.nilWithNilError)-1
			}
		}
		if errNil {
			if out == nil {
				out = make([]bool, len(ret.Results))
			}
			out[i] = true
		}
	}
	return out
}

// boundaryAxioms: Lisette passes pointers and interfaces as non-nil Ref<T>
// and interface values. A Lisette empty slice may be a nil Go slice.
func (a *NilnessAnalysis) boundaryAxioms(fn *ssa.Function, nilableParams []string) []nilnessFact {
	var out []nilnessFact
	for _, param := range fn.Params {
		if slices.Contains(nilableParams, param.Name()) {
			continue
		}
		switch param.Type().Underlying().(type) {
		case *types.Pointer, *types.Interface:
			out = append(out, nilnessFact{param, nilnessNonNil})
		}
	}
	return out
}
