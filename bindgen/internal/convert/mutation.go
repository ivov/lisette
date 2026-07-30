// SSA parameter-mutation analysis: which parameters a function writes through.
package convert

import (
	"go/constant"
	"go/token"
	"go/types"
	"sort"

	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
)

// FunctionMutation is indexed by Go signature parameter; a receiver is reported separately in ReceiverMutates.
type FunctionMutation struct {
	Params          []bool
	ReceiverMutates bool
}

func (m FunctionMutation) Mutates(index int) bool {
	return index < len(m.Params) && m.Params[index]
}

// MutationAnalysis is precomputed at construction and read-only afterwards,
// since `generate_std` converts packages concurrently.
type MutationAnalysis struct {
	program    *ssa.Program
	verdicts   map[*types.Func]FunctionMutation
	summaries  map[*ssa.Function]*mutationSummary
	inProgress map[*ssa.Function]bool
	// sawCycle: a summary was read before it was finished, so settle must run.
	sawCycle bool
}

// mutationSummary is keyed by SSA parameter index (a receiver is 0), and by
// free-variable index for closures.
type mutationSummary struct {
	params   map[int]reachMode
	freeVars map[int]reachMode
}

// reachMode says whether a write replaced what the parameter points at or wrote
// inside it. A parameter can be reached both ways on different paths.
type reachMode uint8

const (
	reachDirect reachMode = 1 << iota
	reachThroughLoad
)

func modeFor(throughLoad bool) reachMode {
	if throughLoad {
		return reachThroughLoad
	}
	return reachDirect
}

// weight sums the modes, so a parameter that gains a second mode counts as
// growth and not only a newly found parameter.
func (m *mutationSummary) weight() int {
	total := 0
	for _, mode := range m.params {
		total += int(mode)
	}
	for _, mode := range m.freeVars {
		total += int(mode)
	}
	return total
}

// NewMutationAnalysis reuses the SSA program the nilability analysis built.
func NewMutationAnalysis(nilness *NilnessAnalysis, roots []*packages.Package) *MutationAnalysis {
	if nilness == nil || nilness.program == nil {
		return nil
	}
	analysis := &MutationAnalysis{
		program:    nilness.program,
		verdicts:   make(map[*types.Func]FunctionMutation),
		summaries:  make(map[*ssa.Function]*mutationSummary),
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

	// Read verdicts only after settling, so a function bound early does not
	// freeze an answer a later pass grows.
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

func (a *MutationAnalysis) Function(obj types.Object) (FunctionMutation, bool) {
	if a == nil {
		return FunctionMutation{}, false
	}
	fn, ok := obj.(*types.Func)
	if !ok {
		return FunctionMutation{}, false
	}
	facts, ok := a.verdicts[fn]
	return facts, ok
}

func (a *MutationAnalysis) record(fn *types.Func) {
	if _, done := a.verdicts[fn]; done {
		return
	}
	sig, ok := fn.Type().(*types.Signature)
	if !ok {
		return
	}
	paramCount := sig.Params().Len()
	facts := FunctionMutation{Params: make([]bool, paramCount)}

	ssaFn := a.program.FuncValue(fn)
	if ssaFn == nil || functionBody(ssaFn) == nil {
		a.verdicts[fn] = facts // the curated map and the name heuristic decide
		return
	}

	// An SSA method carries its receiver as parameter 0.
	offset := 0
	if sig.Recv() != nil {
		offset = 1
	}
	summary := a.summarize(ssaFn)
	if sig.Recv() != nil {
		if _, wrote := summary.params[0]; wrote {
			facts.ReceiverMutates = true
		}
	}
	for index := range summary.params {
		if signatureIndex := index - offset; signatureIndex >= 0 && signatureIndex < paramCount {
			facts.Params[signatureIndex] = true
		}
	}
	a.verdicts[fn] = facts
}

func (a *MutationAnalysis) summarize(fn *ssa.Function) *mutationSummary {
	if s, ok := a.summaries[fn]; ok {
		if a.inProgress[fn] {
			a.sawCycle = true // s holds only what has been found so far
		}
		return s
	}
	summary := &mutationSummary{params: map[int]reachMode{}, freeVars: map[int]reachMode{}}
	a.summaries[fn] = summary
	a.walk(fn, summary)
	return summary
}

// walk grows `summary` in place, so re-running it only adds.
func (a *MutationAnalysis) walk(fn *ssa.Function, summary *mutationSummary) {
	body := functionBody(fn)
	if body == nil {
		return
	}
	a.inProgress[fn] = true
	defer delete(a.inProgress, fn)
	for _, block := range body.Blocks {
		for _, instruction := range block.Instrs {
			a.recordWrites(body, instruction, summary)
		}
	}
}

// settle re-walks every summary until none grows, which is what makes two
// mutually recursive functions agree: whichever was summarized first read the
// other while it was still empty. It runs until nothing changes rather than for
// a fixed number of passes, since stopping early would leave the answer
// depending on which function was walked first. Findings are only ever added,
// so this terminates.
func (a *MutationAnalysis) settle() {
	if !a.sawCycle {
		return
	}
	for grew := true; grew; {
		grew = false
		for _, fn := range a.summarized() {
			summary := a.summaries[fn]
			before := summary.weight()
			a.walk(fn, summary)
			if summary.weight() != before {
				grew = true
			}
		}
	}
}

// summarized snapshots the keys, since a walk can add a callee to the map.
func (a *MutationAnalysis) summarized() []*ssa.Function {
	out := make([]*ssa.Function, 0, len(a.summaries))
	for fn := range a.summaries {
		out = append(out, fn)
	}
	return out
}

func (a *MutationAnalysis) recordWrites(fn *ssa.Function, instruction ssa.Instruction, summary *mutationSummary) {
	switch typed := instruction.(type) {
	case *ssa.Store:
		// Overwriting a local or captured variable is not a write the caller can
		// see. SSA stores every captured parameter into one of these.
		if isVariableSlot(typed.Addr) {
			return
		}
		markRoots(fn, typed.Addr, summary)
	case *ssa.MapUpdate:
		markRoots(fn, typed.Map, summary)
	case *ssa.MakeClosure:
		a.recordClosureWrites(fn, typed, summary)
	case ssa.CallInstruction:
		a.recordCallWrites(fn, typed.Common(), summary)
	}
}

// recordClosureWrites assumes the closure runs, since it cannot know who calls it.
func (a *MutationAnalysis) recordClosureWrites(fn *ssa.Function, closure *ssa.MakeClosure, summary *mutationSummary) {
	target, ok := closure.Fn.(*ssa.Function)
	if !ok {
		return
	}
	inner := a.summarize(target)
	for index, mode := range inner.freeVars {
		if index < len(closure.Bindings) {
			resumeWalk(fn, closure.Bindings[index], summary, mode)
		}
	}
}

// resumeWalk continues the way the callee reached its own parameter, so
// `write(&local)` reaches what `local` holds while `advance(&cursor)` does not.
func resumeWalk(fn *ssa.Function, value ssa.Value, summary *mutationSummary, mode reachMode) {
	if mode&reachDirect != 0 {
		markRoots(fn, value, summary)
	}
	if mode&reachThroughLoad != 0 {
		markRootsThroughLoad(fn, value, summary)
	}
}

func (a *MutationAnalysis) recordCallWrites(fn *ssa.Function, common *ssa.CallCommon, summary *mutationSummary) {
	if builtin, ok := common.Value.(*ssa.Builtin); ok {
		switch builtin.Name() {
		case "copy", "delete", "clear":
			if len(common.Args) > 0 {
				markRoots(fn, common.Args[0], summary)
			}
		case "append":
			// Writes past the length of its first argument when capacity
			// allowed, so `append(s[:0], v)` overwrites `s[0]`.
			if len(common.Args) > 0 && !appendMustReallocate(common.Args[0]) {
				markRoots(fn, common.Args[0], summary)
			}
		}
		return
	}
	// A dynamic call has no body to consult, and a bodiless callee summarizes as
	// writing nothing. Assuming otherwise for either was measured and rejected.
	callee := common.StaticCallee()
	if callee == nil {
		return
	}
	inner := a.summarize(callee)
	for index, mode := range inner.params {
		if index < len(common.Args) {
			resumeWalk(fn, common.Args[index], summary, mode)
		}
	}
}

func markRoots(fn *ssa.Function, value ssa.Value, summary *mutationSummary) {
	markRootsSeen(fn, value, summary, false, make(map[rootVisit]bool))
}

// markRootsThroughLoad starts as though a load stood before `value`.
func markRootsThroughLoad(fn *ssa.Function, value ssa.Value, summary *mutationSummary) {
	markRootsSeen(fn, value, summary, true, make(map[rootVisit]bool))
}

// rootVisit keys the visited set, since the two modes give different answers.
type rootVisit struct {
	value       ssa.Value
	throughLoad bool
}

// markRootsSeen walks back from a written location to the parameters and
// captured variables that the write actually lands in.
func markRootsSeen(fn *ssa.Function, value ssa.Value, summary *mutationSummary, throughLoad bool, seen map[rootVisit]bool) {
	visit := rootVisit{value, throughLoad}
	if value == nil || seen[visit] {
		return
	}
	seen[visit] = true
	descend := func(next ssa.Value, loaded bool) {
		markRootsSeen(fn, next, summary, loaded, seen)
	}

	switch typed := value.(type) {
	case *ssa.Parameter:
		for index, param := range fn.Params {
			if param == typed {
				summary.params[index] |= modeFor(throughLoad)
				return
			}
		}
	case *ssa.FreeVar:
		for index, free := range fn.FreeVars {
			if free == typed {
				summary.freeVars[index] |= modeFor(throughLoad)
				return
			}
		}
	case *ssa.Slice:
		descend(typed.X, throughLoad)
	case *ssa.IndexAddr:
		descend(typed.X, throughLoad)
	case *ssa.FieldAddr:
		descend(typed.X, throughLoad)
	case *ssa.Field:
		// Copying a struct or array copies the slice headers inside it. SSA uses this
		// value form only where it cannot take an address, such as a call result, so
		// nothing reaches it until call results are tracked.
		if !isIndirection(typed.Type()) {
			descend(typed.X, throughLoad)
		}
	case *ssa.Index:
		if !isIndirection(typed.Type()) {
			descend(typed.X, throughLoad)
		}
	case *ssa.ChangeType:
		descend(typed.X, throughLoad)
	case *ssa.Convert:
		// A string conversion allocates, so `[]byte(string(src))` is storage
		// of its own. Pointer and numeric conversions keep it.
		if !convertCopies(typed.X.Type(), typed.Type()) {
			descend(typed.X, throughLoad)
		}
	case *ssa.SliceToArrayPointer:
		descend(typed.X, throughLoad)
	case *ssa.Lookup:
		if !typed.CommaOk && !isIndirection(typed.Type()) {
			descend(typed.X, throughLoad)
		}
	case *ssa.MakeInterface:
		descend(typed.X, throughLoad)
	case *ssa.ChangeInterface:
		descend(typed.X, throughLoad)
	case *ssa.TypeAssert:
		descend(typed.X, throughLoad)
	case *ssa.Extract:
		// The comma-ok and range forms hand back a tuple whose value still points at
		// the map or interface it came out of.
		switch tuple := typed.Tuple.(type) {
		case *ssa.TypeAssert:
			if typed.Index == 0 {
				descend(tuple.X, throughLoad)
			}
		case *ssa.Lookup:
			if typed.Index == 0 && !isIndirection(typed.Type()) {
				descend(tuple.X, throughLoad)
			}
		case *ssa.Next:
			if rang, ok := tuple.Iter.(*ssa.Range); ok &&
				typed.Index == 2 && !isIndirection(typed.Type()) {
				descend(rang.X, throughLoad)
			}
		}
	case *ssa.Phi:
		for _, edge := range typed.Edges {
			descend(edge, throughLoad)
		}
	case *ssa.UnOp:
		if typed.Op != token.MUL {
			return
		}
		// A pointer or interface read out of a slice or map ends the trail, since the
		// slice holds only the pointer and the write lands on what it points at. Read
		// out of a local variable it does not, which keeps `v := any(s)` reaching `s`.
		if isIndirection(typed.Type()) && !isVariableSlot(typed.X) {
			return
		}
		descend(typed.X, true)
	case *ssa.Alloc:
		// Arriving through a load means the write went inside whatever the local
		// holds, so keep going. Arriving directly means the local itself was
		// overwritten, which says nothing about where its value came from.
		if !throughLoad {
			return
		}
		for _, stored := range storesInto(typed) {
			descend(stored, false)
		}
	case *ssa.Call:
		// An append result shares the argument's backing array, which is how
		// `slices.Delete` becomes visible: it shifts elements down, then clears the
		// leftover tail. Only the zero-capacity form breaks that, since appending
		// nothing to a full slice hands the same slice straight back.
		if builtin, ok := typed.Common().Value.(*ssa.Builtin); ok &&
			builtin.Name() == "append" && len(typed.Common().Args) > 0 &&
			!isZeroCapacity(typed.Common().Args[0]) {
			descend(typed.Common().Args[0], throughLoad)
		}
	}
}

func convertCopies(from, to types.Type) bool {
	return isStringType(from) || isStringType(to)
}

func isStringType(t types.Type) bool {
	basic, ok := t.Underlying().(*types.Basic)
	return ok && basic.Info()&types.IsString != 0
}

// isIndirection reports the types that refer to a separate object rather than
// holding one. A type parameter needs its core type first, since `Underlying` on
// one yields the constraint interface, which would stop every generic slice.
func isIndirection(t types.Type) bool {
	if parameter, ok := t.(*types.TypeParam); ok {
		core := coreType(parameter)
		if core == nil {
			return false // several shapes admitted: keep walking
		}
		t = core
	}
	switch t.Underlying().(type) {
	case *types.Pointer, *types.Interface:
		return true
	}
	return false
}

// coreType returns the underlying type a constraint's members share, or nil.
func coreType(parameter *types.TypeParam) types.Type {
	return constraintCore(parameter.Constraint(), make(map[types.Type]bool))
}

// constraintCore recurses, since a constraint may embed another named interface
// rather than stating its terms directly.
func constraintCore(constraint types.Type, seen map[types.Type]bool) types.Type {
	iface, ok := constraint.Underlying().(*types.Interface)
	if !ok {
		return constraint.Underlying()
	}
	if seen[constraint] {
		return nil
	}
	seen[constraint] = true

	var core types.Type
	for i := range iface.NumEmbeddeds() {
		for _, term := range unionTerms(iface.EmbeddedType(i)) {
			resolved := constraintCore(term, seen)
			if resolved == nil {
				return nil
			}
			if core == nil {
				core = resolved
				continue
			}
			if !types.Identical(core, resolved) {
				return nil
			}
		}
	}
	return core // nil when only methods are listed, as `any` does
}

func unionTerms(embedded types.Type) []types.Type {
	union, ok := embedded.(*types.Union)
	if !ok {
		return []types.Type{embedded}
	}
	out := make([]types.Type, 0, union.Len())
	for i := range union.Len() {
		out = append(out, union.Term(i).Type())
	}
	return out
}

// appendMustReallocate reports the two idioms with no spare room: zero-capacity
// `s[:0:0]`, and the filled-to-capacity `s[:cap(s)]` that `slices.Grow` uses.
func appendMustReallocate(value ssa.Value) bool {
	return isZeroCapacity(value) || isFilledToCapacity(value)
}

func isFilledToCapacity(value ssa.Value) bool {
	sliced, ok := value.(*ssa.Slice)
	if !ok || sliced.Max != nil || sliced.High == nil {
		return false
	}
	call, ok := sliced.High.(*ssa.Call)
	if !ok {
		return false
	}
	builtin, ok := call.Common().Value.(*ssa.Builtin)
	return ok && builtin.Name() == "cap" &&
		len(call.Common().Args) == 1 && call.Common().Args[0] == sliced.X
}

func isZeroCapacity(value ssa.Value) bool {
	sliced, ok := value.(*ssa.Slice)
	if !ok || sliced.Max == nil {
		return false
	}
	max, ok := constantIndex(sliced.Max)
	if !ok {
		return false
	}
	if sliced.Low == nil {
		return max == 0
	}
	low, ok := constantIndex(sliced.Low)
	return ok && low == max
}

func constantIndex(value ssa.Value) (int64, bool) {
	konst, ok := value.(*ssa.Const)
	if !ok || konst.Value == nil || konst.Value.Kind() != constant.Int {
		return 0, false
	}
	return constant.Int64Val(konst.Value)
}

// isVariableSlot reports an address where writing overwrites a local or captured
// variable, rather than writing inside the value it holds.
func isVariableSlot(address ssa.Value) bool {
	switch address.(type) {
	case *ssa.Alloc, *ssa.FreeVar:
		return true
	}
	return false
}

func storesInto(alloc *ssa.Alloc) []ssa.Value {
	referrers := alloc.Referrers()
	if referrers == nil {
		return nil
	}
	var out []ssa.Value
	for _, ref := range *referrers {
		if store, ok := ref.(*ssa.Store); ok && store.Addr == alloc {
			out = append(out, store.Val)
		}
	}
	return out
}
