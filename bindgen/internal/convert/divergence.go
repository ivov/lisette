// SSA divergence analysis: which functions never return normally.
package convert

import (
	"go/types"
	"sort"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/config"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
)

// DivergenceAnalysis is precomputed once and read-only afterwards.
type DivergenceAnalysis struct {
	program    *ssa.Program
	cfg        *config.Config
	verdicts   map[*types.Func]bool
	summaries  map[*ssa.Function]bool
	inProgress map[*ssa.Function]bool
	sawCycle   bool
}

// NewDivergenceAnalysis reuses the SSA program the nilability analysis built.
func NewDivergenceAnalysis(nilness *NilnessAnalysis, roots []*packages.Package, cfg *config.Config) *DivergenceAnalysis {
	if nilness == nil || nilness.program == nil {
		return nil
	}
	analysis := &DivergenceAnalysis{
		program:    nilness.program,
		cfg:        cfg,
		verdicts:   make(map[*types.Func]bool),
		summaries:  make(map[*ssa.Function]bool),
		inProgress: make(map[*ssa.Function]bool),
	}

	wellTyped := make([]*packages.Package, 0, len(roots))
	for _, pkg := range roots {
		if pkg != nil && pkg.Types != nil && len(pkg.Errors) == 0 {
			wellTyped = append(wellTyped, pkg)
		}
	}
	sort.Slice(wellTyped, func(i, j int) bool { return wellTyped[i].PkgPath < wellTyped[j].PkgPath })

	var bound []*types.Func
	for _, pkg := range wellTyped {
		scope := pkg.Types.Scope()
		for _, name := range scope.Names() {
			switch obj := scope.Lookup(name).(type) {
			case *types.Func:
				bound = append(bound, obj)
			case *types.TypeName:
				named, ok := obj.Type().(*types.Named)
				if !ok {
					continue
				}
				for sel := range analysis.program.MethodSets.MethodSet(types.NewPointer(named)).Methods() {
					if fn, ok := sel.Obj().(*types.Func); ok {
						bound = append(bound, fn)
					}
				}
			}
		}
	}

	for _, fn := range bound {
		if ssaFn := analysis.program.FuncValue(fn); ssaFn != nil && functionBody(ssaFn) != nil {
			analysis.summarize(ssaFn)
		}
	}
	analysis.settle()
	for _, fn := range bound {
		analysis.record(fn)
	}
	return analysis
}

// Function reports whether obj never returns normally, if derivable.
func (a *DivergenceAnalysis) Function(obj types.Object) (bool, bool) {
	if a == nil {
		return false, false
	}
	fn, ok := obj.(*types.Func)
	if !ok {
		return false, false
	}
	diverges, ok := a.verdicts[fn]
	return diverges, ok
}

func (a *DivergenceAnalysis) record(fn *types.Func) {
	if _, done := a.verdicts[fn]; done {
		return
	}
	ssaFn := a.program.FuncValue(fn)
	if ssaFn == nil {
		a.verdicts[fn] = a.axiomDiverges(fn)
		return
	}
	a.verdicts[fn] = a.summarize(ssaFn)
}

func (a *DivergenceAnalysis) summarize(fn *ssa.Function) bool {
	if s, ok := a.summaries[fn]; ok {
		if a.inProgress[fn] {
			a.sawCycle = true
		}
		return s
	}
	a.summaries[fn] = false
	a.inProgress[fn] = true
	result := a.computeDiverges(fn)
	delete(a.inProgress, fn)
	a.summaries[fn] = result
	return result
}

// settle re-walks every summary until none changes; facts only flip false to true.
func (a *DivergenceAnalysis) settle() {
	if !a.sawCycle {
		return
	}
	for grew := true; grew; {
		grew = false
		for _, fn := range a.summarized() {
			before := a.summaries[fn]
			after := a.computeDiverges(fn)
			if after != before {
				a.summaries[fn] = after
				grew = true
			}
		}
	}
}

func (a *DivergenceAnalysis) summarized() []*ssa.Function {
	out := make([]*ssa.Function, 0, len(a.summaries))
	for fn := range a.summaries {
		out = append(out, fn)
	}
	return out
}

func (a *DivergenceAnalysis) computeDiverges(fn *ssa.Function) bool {
	if obj, ok := fn.Object().(*types.Func); ok && a.axiomDiverges(obj) {
		return true
	}
	body := functionBody(fn)
	if body == nil {
		return false
	}
	if body.Recover != nil {
		return false // a deferred call, static or dynamic, might recover the panic
	}
	return !a.reachesReturn(body.Blocks[0], make(map[*ssa.BasicBlock]bool))
}

func isBuiltinDivergenceAxiom(fn *types.Func) bool {
	pkg := fn.Pkg()
	if pkg == nil {
		return false
	}
	switch pkg.Path() {
	case "syscall":
		return fn.Name() == "Exit"
	case "runtime":
		return fn.Name() == "Goexit"
	}
	return false
}

func (a *DivergenceAnalysis) axiomDiverges(fn *types.Func) bool {
	if isBuiltinDivergenceAxiom(fn) {
		return true
	}
	if a.cfg == nil || fn.Pkg() == nil {
		return false
	}
	return a.cfg.IsNeverReturn(fn.Pkg().Path(), qualifiedFunctionName(fn))
}

func (a *DivergenceAnalysis) reachesReturn(block *ssa.BasicBlock, visited map[*ssa.BasicBlock]bool) bool {
	if visited[block] {
		return false
	}
	visited[block] = true
	for _, instr := range block.Instrs {
		switch typed := instr.(type) {
		case *ssa.Call:
			if a.callDiverges(typed) {
				return false
			}
		case *ssa.Return:
			return true
		}
	}
	for _, succ := range block.Succs {
		if a.reachesReturn(succ, visited) {
			return true
		}
	}
	return false
}

func (a *DivergenceAnalysis) callDiverges(call *ssa.Call) bool {
	common := call.Common()
	if common.IsInvoke() {
		return false
	}
	callee := common.StaticCallee()
	if callee == nil {
		return false
	}
	if obj, ok := callee.Object().(*types.Func); ok && obj.Pkg() != nil && isInternalPackage(obj.Pkg().Path()) {
		return false // e.g. internal/abi.EscapeNonString's body always panics but the real intrinsic doesn't
	}
	return a.summarize(callee)
}

func isInternalPackage(path string) bool {
	return path == "internal" || strings.HasPrefix(path, "internal/") || strings.Contains(path, "/internal/")
}
