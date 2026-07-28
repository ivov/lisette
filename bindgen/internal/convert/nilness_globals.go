// Package-level variable invariants.
package convert

import (
	"go/token"
	"sort"

	"golang.org/x/tools/go/ssa"
	"golang.org/x/tools/go/ssa/ssautil"
)

type globalUsage struct {
	stores  []globalStore
	escaped bool // address used beyond direct loads and stores
}

type globalStore struct {
	value ssa.Value
	// declaredInitializer: unconditional store in the owning package's init
	declaredInitializer bool
}

// globalNilness needs a non-nil initializer, all stores non-nil, and no
// address escape. Exported variables are writable from Lisette.
func (a *NilnessAnalysis) globalNilness(g *ssa.Global) nilness {
	obj := g.Object()
	if obj == nil || obj.Exported() || g.Pkg == nil {
		return nilnessUnknown
	}
	if fact, ok := a.globalFacts[g]; ok {
		return fact
	}
	a.sweepGlobals()

	usage := a.globalUsages[g]
	if usage == nil || usage.escaped {
		a.globalFacts[g] = nilnessUnknown
		return nilnessUnknown
	}

	a.globalFacts[g] = nilnessUnknown // seed for initializer cycles

	fact := nilnessNonNil
	hasDeclaredInitializer := false
	for _, store := range usage.stores {
		if store.declaredInitializer {
			hasDeclaredInitializer = true
		}
		if a.eval(store.value, nil, make(map[ssa.Value]bool)).n != nilnessNonNil {
			fact = nilnessUnknown
			break
		}
	}
	if !hasDeclaredInitializer {
		fact = nilnessUnknown
	}
	a.globalFacts[g] = fact
	return fact
}

// sweepGlobals indexes all Global stores and escapes, once. Functions are
// sorted first because ssautil.AllFunctions iterates in map order.
func (a *NilnessAnalysis) sweepGlobals() {
	if a.globalUsages != nil {
		return
	}
	a.globalUsages = make(map[*ssa.Global]*globalUsage)

	usageFor := func(g *ssa.Global) *globalUsage {
		u := a.globalUsages[g]
		if u == nil {
			u = &globalUsage{}
			a.globalUsages[g] = u
		}
		return u
	}

	functions := make([]*ssa.Function, 0, 1024)
	for fn := range ssautil.AllFunctions(a.program) {
		functions = append(functions, fn)
	}
	names := make(map[*ssa.Function]string, len(functions))
	for _, fn := range functions {
		names[fn] = fn.String()
	}
	sort.Slice(functions, func(i, j int) bool { return names[functions[i]] < names[functions[j]] })

	var operands []*ssa.Value
	for _, fn := range functions {
		for _, block := range fn.Blocks {
			for _, instr := range block.Instrs {
				operands = instr.Operands(operands[:0])
				for _, operand := range operands {
					g, ok := (*operand).(*ssa.Global)
					if !ok {
						continue
					}
					switch use := instr.(type) {
					case *ssa.Store:
						if use.Addr == g && use.Val != g {
							usageFor(g).stores = append(usageFor(g).stores, globalStore{
								value:               use.Val,
								declaredInitializer: isDeclaredInitializerStore(fn, g),
							})
							continue
						}
						usageFor(g).escaped = true
					case *ssa.UnOp:
						if use.Op != token.MUL || use.X != g {
							usageFor(g).escaped = true
						}
					default:
						usageFor(g).escaped = true
					}
				}
			}
		}
	}

}

func isDeclaredInitializerStore(fn *ssa.Function, g *ssa.Global) bool {
	return fn.Pkg != nil && g.Pkg != nil && fn.Pkg == g.Pkg && fn.Name() == "init"
}
