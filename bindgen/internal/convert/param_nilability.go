// SSA parameter-nilability analysis: which parameters a function accepts as nil.
package convert

import (
	"go/token"
	"go/types"
	"slices"

	"github.com/ivov/lisette/bindgen/internal/config"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
)

// FunctionParamNilability is indexed by Go signature parameter.
type FunctionParamNilability struct {
	ToleratesNil []bool
}

func (f FunctionParamNilability) Tolerates(index int) bool {
	return index < len(f.ToleratesNil) && f.ToleratesNil[index]
}

// ParameterNilability is precomputed before the return verdicts that read it,
// and read-only afterwards, since `generate_std` converts concurrently.
type ParameterNilability struct {
	nilness  *NilnessAnalysis
	cfg      *config.Config
	verdicts map[*types.Func]FunctionParamNilability
	shapes   declaredShapes
}

// declaredShapes are signatures written down elsewhere: interface methods and
// named function types. Both lack a body, so their parameters never flip, and
// flipping an implementation alone breaks the match.
type declaredShapes struct {
	methods   map[string][]*types.Signature // by method name
	funcTypes []*types.Signature            // e.g. http.HandlerFunc
}

func newParameterNilability(nilness *NilnessAnalysis, bound []*types.Func, pkgs []*packages.Package) *ParameterNilability {
	analysis := &ParameterNilability{
		nilness:  nilness,
		cfg:      nilness.cfg,
		verdicts: make(map[*types.Func]FunctionParamNilability),
		shapes:   indexDeclaredShapes(pkgs),
	}
	for _, fn := range bound {
		analysis.record(fn)
	}
	return analysis
}

// indexDeclaredShapes walks the roots and everything they import, since the
// shape is usually declared in another package.
func indexDeclaredShapes(pkgs []*packages.Package) declaredShapes {
	out := declaredShapes{methods: make(map[string][]*types.Signature)}
	seen := make(map[string]bool)

	var visit func(pkg *packages.Package)
	visit = func(pkg *packages.Package) {
		if pkg == nil || pkg.Types == nil || seen[pkg.PkgPath] {
			return
		}
		seen[pkg.PkgPath] = true

		scope := pkg.Types.Scope()
		for _, name := range scope.Names() {
			obj, ok := scope.Lookup(name).(*types.TypeName)
			if !ok {
				continue
			}
			switch underlying := obj.Type().Underlying().(type) {
			case *types.Interface:
				for method := range underlying.Methods() {
					if sig, ok := method.Type().(*types.Signature); ok {
						out.methods[method.Name()] = append(out.methods[method.Name()], sig)
					}
				}
			case *types.Signature:
				out.funcTypes = append(out.funcTypes, underlying)
			}
		}
		for _, imported := range pkg.Imports {
			visit(imported)
		}
	}
	for _, pkg := range pkgs {
		visit(pkg)
	}
	return out
}

// hasDeclaredShape tests signature identity rather than `types.Implements` on
// the receiver, since a method usually satisfies an interface through the
// struct embedding it. It over-rejects a coincidental match.
func (a *ParameterNilability) hasDeclaredShape(fn *types.Func) bool {
	sig, ok := fn.Type().(*types.Signature)
	if !ok {
		return false
	}
	bare := types.NewSignatureType(nil, nil, nil, sig.Params(), sig.Results(), sig.Variadic())
	for _, declared := range a.shapes.methods[fn.Name()] {
		if types.Identical(bare, declared) {
			return true
		}
	}
	for _, declared := range a.shapes.funcTypes {
		if types.Identical(bare, declared) {
			return true
		}
	}
	return false
}

func (a *ParameterNilability) Function(obj types.Object) (FunctionParamNilability, bool) {
	if a == nil {
		return FunctionParamNilability{}, false
	}
	fn, ok := obj.(*types.Func)
	if !ok {
		return FunctionParamNilability{}, false
	}
	facts, ok := a.verdicts[fn]
	return facts, ok
}

// Optional is shared by the conversion and the axiom seed, and answers from
// the config this analysis was built with, so neither caller can supply one the
// other did not see.
func (a *ParameterNilability) Optional(fn *types.Func, index int) bool {
	if a == nil {
		return false
	}
	sig, ok := fn.Type().(*types.Signature)
	if !ok || index < 0 || index >= sig.Params().Len() {
		return false
	}
	param := sig.Params().At(index)
	if !isNilableGoType(param.Type()) {
		return false
	}
	if sig.Variadic() && index == sig.Params().Len()-1 {
		return false
	}
	if forced, decided := a.configOptional(fn, index); decided {
		return forced
	}
	if a.hasDeclaredShape(fn) {
		return false
	}
	verdict, _ := a.Function(fn)
	return verdict.Tolerates(index)
}

// configOptional answers from config alone, reporting whether it decided.
// Overrides sit under the declaring type. The return pin runs before
// `nilable_param`, or an override would slip past it.
func (a *ParameterNilability) configOptional(fn *types.Func, index int) (optional, decided bool) {
	cfg := a.cfg
	if cfg == nil || fn.Pkg() == nil {
		return false, false
	}
	sig, ok := fn.Type().(*types.Signature)
	if !ok || index < 0 || index >= sig.Params().Len() {
		return false, false
	}
	name := sig.Params().At(index).Name()
	pkg, key := fn.Pkg().Path(), qualifiedFunctionName(fn)
	if name != "" && slices.Contains(cfg.NonNilableParams(pkg, key), name) {
		return false, true
	}
	if cfg.IsNonNilableReturn(pkg, key) {
		return false, true
	}
	if name != "" && slices.Contains(cfg.NilableParams(pkg, key), name) {
		return true, true
	}
	return false, false
}

func (a *ParameterNilability) record(fn *types.Func) {
	if _, done := a.verdicts[fn]; done {
		return
	}
	sig, ok := fn.Type().(*types.Signature)
	if !ok {
		return
	}
	facts := FunctionParamNilability{ToleratesNil: make([]bool, sig.Params().Len())}

	ssaFn := a.nilness.program.FuncValue(fn)
	if ssaFn == nil {
		a.verdicts[fn] = facts
		return
	}
	body := functionBody(ssaFn)
	if body == nil {
		a.verdicts[fn] = facts
		return
	}

	// An SSA method carries its receiver as parameter 0, which is skipped.
	offset := 0
	if sig.Recv() != nil {
		offset = 1
	}

	// A forced parameter never walks, so neighbours prove against it being nil.
	var forced []*ssa.Parameter
	for index := range facts.ToleratesNil {
		if optional, decided := a.configOptional(fn, index); decided && optional {
			if position := index + offset; position < len(body.Params) {
				forced = append(forced, body.Params[position])
			}
		}
	}

	for index := range facts.ToleratesNil {
		if sig.Variadic() && index == sig.Params().Len()-1 {
			continue
		}
		position := index + offset
		if position >= len(body.Params) {
			continue
		}
		param := body.Params[position]
		if !isNilableGoType(param.Type()) {
			continue
		}
		facts.ToleratesNil[index] = a.provenTolerant(body, param, forced)
	}
	a.verdicts[fn] = facts
}

// provenTolerant pins `param` nil and leaves neighbours unconstrained, so the
// proof holds for any combination of None arguments. Any rejection vetoes.
func (a *ParameterNilability) provenTolerant(body *ssa.Function, param *ssa.Parameter, forced []*ssa.Parameter) bool {
	axioms := []nilnessFact{{value: param, n: nilnessNil}}
	mayNil := append([]*ssa.Parameter{param}, forced...)
	reject := false
	ordinary := false

	a.nilness.walk(body, axioms, func(instr ssa.Instruction, dominating []nilnessFact) {
		if reject {
			return
		}
		if ret, ok := instr.(*ssa.Return); ok {
			if isOrdinaryReturn(body, ret) {
				ordinary = true
			} else {
				reject = true
			}
			return
		}
		if a.rejects(instr, dominating, mayNil) {
			reject = true
		}
	})

	return !reject && ordinary
}

// rejects is default-deny: touching the tracked value rejects unless `safeUse`
// allows it, since a forgotten dangerous form would read as tolerance. A panic
// is judged by reachability, having no provenance to attribute it.
func (a *ParameterNilability) rejects(instr ssa.Instruction, dominating []nilnessFact, mayNil []*ssa.Parameter) bool {
	if _, isPanic := instr.(*ssa.Panic); isPanic {
		return true
	}
	if closure, ok := instr.(*ssa.MakeClosure); ok {
		for _, binding := range closure.Bindings {
			if a.bindingCarries(binding, dominating, mayNil) {
				return true
			}
		}
	}
	if !a.touches(instr, dominating, mayNil) {
		return false
	}
	return !a.safeUse(instr, dominating, mayNil)
}

func (a *ParameterNilability) touches(instr ssa.Instruction, dominating []nilnessFact, mayNil []*ssa.Parameter) bool {
	var operands [8]*ssa.Value
	for _, operand := range instr.Operands(operands[:0]) {
		if operand == nil || *operand == nil {
			continue
		}
		if a.carries(*operand, dominating, mayNil, make(map[ssa.Value]bool)) {
			return true
		}
	}
	return false
}

// safeUse lists what a possibly-nil value tolerates: cannot fault on nil, or
// yields a value `carries` keeps tracking.
func (a *ParameterNilability) safeUse(instr ssa.Instruction, dominating []nilnessFact, mayNil []*ssa.Parameter) bool {
	switch typed := instr.(type) {
	case *ssa.BinOp:
		return typed.Op == token.EQL || typed.Op == token.NEQ

	case *ssa.Phi, *ssa.ChangeInterface, *ssa.ChangeType, *ssa.Convert, *ssa.MakeInterface,
		*ssa.Extract, *ssa.Return, *ssa.DebugRef:
		return true

	case *ssa.Lookup:
		return true // nil map reads yield the zero value, nil keys are ordinary

	case *ssa.TypeAssert:
		return typed.CommaOk // failure yields the zero value instead of panicking

	case *ssa.Store:
		// Writing through the parameter dereferences it, and a heap cell outlives
		// the call.
		return !a.carries(typed.Addr, dominating, mayNil, make(map[ssa.Value]bool)) && isFrameSlot(typed.Addr)

	case *ssa.UnOp:
		return typed.Op == token.MUL && isFrameSlot(typed.X) // load back out of a spill

	case ssa.CallInstruction:
		builtin, ok := typed.Common().Value.(*ssa.Builtin)
		if !ok {
			return false // any call the parameter reaches ends the proof
		}
		return builtin.Name() == "len" || builtin.Name() == "cap"
	}
	return false
}

// isFrameSlot reports a cell that dies with the frame.
func isFrameSlot(v ssa.Value) bool {
	alloc, ok := v.(*ssa.Alloc)
	return ok && !alloc.Heap
}

// carries reports whether v may hold a tracked nil. Evaluation decides it when
// conclusive, which lets `if p == nil { p = &T{} }` pass.
func (a *ParameterNilability) carries(v ssa.Value, dominating []nilnessFact, mayNil []*ssa.Parameter, seen map[ssa.Value]bool) bool {
	if v == nil || seen[v] {
		return false
	}
	seen[v] = true
	switch a.nilness.eval(v, dominating, make(map[ssa.Value]bool)).n {
	case nilnessNil:
		return true
	case nilnessNonNil:
		if _, wraps := v.(*ssa.MakeInterface); !wraps {
			return false
		}
	}
	if param, ok := v.(*ssa.Parameter); ok && slices.Contains(mayNil, param) {
		return true
	}

	switch typed := v.(type) {
	case *ssa.MakeInterface:
		return a.carries(typed.X, dominating, mayNil, seen)
	case *ssa.ChangeInterface:
		return a.carries(typed.X, dominating, mayNil, seen)
	case *ssa.ChangeType:
		return a.carries(typed.X, dominating, mayNil, seen)
	case *ssa.Convert:
		return a.carries(typed.X, dominating, mayNil, seen)
	case *ssa.Slice:
		return a.carries(typed.X, dominating, mayNil, seen)
	case *ssa.Phi:
		for _, edge := range typed.Edges {
			if a.carries(edge, dominating, mayNil, seen) {
				return true
			}
		}
	case *ssa.Extract:
		if assert, ok := typed.Tuple.(*ssa.TypeAssert); ok {
			return a.carries(assert.X, dominating, mayNil, seen)
		}
	case *ssa.UnOp:
		if spilled := resolveSpill(typed); spilled != ssa.Value(typed) {
			return a.carries(spilled, dominating, mayNil, seen)
		}
	}
	return false
}

func (a *ParameterNilability) bindingCarries(v ssa.Value, dominating []nilnessFact, mayNil []*ssa.Parameter) bool {
	if a.carries(v, dominating, mayNil, make(map[ssa.Value]bool)) {
		return true
	}
	alloc, ok := v.(*ssa.Alloc)
	if !ok || alloc.Referrers() == nil {
		return false
	}
	for _, ref := range *alloc.Referrers() {
		if store, ok := ref.(*ssa.Store); ok && store.Addr == ssa.Value(alloc) {
			if a.carries(store.Val, dominating, mayNil, make(map[ssa.Value]bool)) {
				return true
			}
		}
	}
	return false
}

// isOrdinaryReturn: no error result, or a nil error constant at this site.
func isOrdinaryReturn(body *ssa.Function, ret *ssa.Return) bool {
	results := body.Signature.Results()
	count := results.Len()
	if count == 0 {
		return true
	}
	if !isErrorType(results.At(count - 1).Type()) {
		return true
	}
	if len(ret.Results) != count {
		return false
	}
	last := ret.Results[count-1]
	return isNilConst(last) || isNilConst(resolveSpill(last))
}
