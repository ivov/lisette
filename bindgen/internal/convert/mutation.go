// SSA parameter-mutation analysis: which parameters a function writes through,
// and which of its results are views of them.
package convert

import (
	"go/constant"
	"go/token"
	"go/types"
	"slices"
	"sort"
	"strconv"
	"strings"

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

// ViewDepth says whether shared storage is the result itself or sits inside it.
type ViewDepth uint8

const (
	DepthNone ViewDepth = iota
	DepthWhole
	DepthElement
)

// joinDepth keeps the outermost sharing.
func joinDepth(a, b ViewDepth) ViewDepth {
	if a == DepthNone {
		return b
	}
	if b == DepthNone {
		return a
	}
	return min(a, b)
}

// FunctionViews is indexed by Go signature result.
type FunctionViews struct {
	Results  []ResultView
	Analyzed bool // false when SSA never saw a body
}

func (v FunctionViews) Fresh() bool {
	for _, result := range v.Results {
		if !result.Fresh() {
			return false
		}
	}
	return true
}

// ResultView is one result's aliasing set. Opaque: the walk met an
// unsupported operation, so the worst case applies unless an override
// completes it. Shared: can expose storage belonging to no argument.
type ResultView struct {
	Params   []ViewDepth // indexed by signature parameter, DepthNone when fresh
	Receiver ViewDepth
	Opaque   bool
	Shared   bool
}

func (v ResultView) Param(index int) ViewDepth {
	if index < len(v.Params) {
		return v.Params[index]
	}
	return DepthNone
}

func (v ResultView) Fresh() bool {
	if v.Opaque || v.Shared || v.Receiver != DepthNone {
		return false
	}
	for _, depth := range v.Params {
		if depth != DepthNone {
			return false
		}
	}
	return true
}

func (v ResultView) clone() ResultView {
	return ResultView{Params: slices.Clone(v.Params), Receiver: v.Receiver, Opaque: v.Opaque, Shared: v.Shared}
}

func (v *ResultView) setParam(index, paramCount int, depth ViewDepth) {
	if index < 0 || index >= paramCount {
		return
	}
	if v.Params == nil {
		v.Params = make([]ViewDepth, paramCount)
	}
	v.Params[index] = joinDepth(v.Params[index], depth)
}

// MutationAnalysis is precomputed at construction and read-only afterwards,
// since `generate_std` converts packages concurrently.
type MutationAnalysis struct {
	program    *ssa.Program
	verdicts   map[*types.Func]FunctionMutation
	views      map[*types.Func]FunctionViews
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
	results  map[int]*aliasSources
	// handOut: what the function passes to callees it cannot name.
	handOut *aliasSources
	// freeVarStores: what the function stores through each captured variable.
	freeVarStores map[int]*captureStores
}

func (m *mutationSummary) captureStoresFor(index int) *captureStores {
	if m.freeVarStores == nil {
		m.freeVarStores = map[int]*captureStores{}
	}
	buckets, ok := m.freeVarStores[index]
	if !ok {
		buckets = &captureStores{}
		m.freeVarStores[index] = buckets
	}
	return buckets
}

// captureStores splits capture writes by destination, since rebinding and
// element stores sit at different depths.
type captureStores struct {
	rebound  *aliasSources
	elements *aliasSources
}

func newAliasSources() *aliasSources {
	return &aliasSources{params: map[int]viewLevel{}, freeVars: map[int]viewLevel{}}
}

func (c *captureStores) reboundSources() *aliasSources {
	if c.rebound == nil {
		c.rebound = newAliasSources()
	}
	return c.rebound
}

func (c *captureStores) elementSources() *aliasSources {
	if c.elements == nil {
		c.elements = newAliasSources()
	}
	return c.elements
}

func (c *captureStores) weight() int {
	total := 0
	if c.rebound != nil {
		total += c.rebound.weight()
	}
	if c.elements != nil {
		total += c.elements.weight()
	}
	return total
}

func (m *mutationSummary) handOutSources() *aliasSources {
	if m.handOut == nil {
		m.handOut = &aliasSources{params: map[int]viewLevel{}, freeVars: map[int]viewLevel{}}
	}
	return m.handOut
}

// aliasSources is indexed by SSA parameter (receiver is 0) and free variable.
type aliasSources struct {
	params   map[int]viewLevel
	freeVars map[int]viewLevel
	opaque   bool
	shared   bool
}

// viewLevel counts steps inside the result, not telling deeper levels apart.
type viewLevel uint8

const (
	levelWhole viewLevel = iota
	levelElement
	levelDeep
)

// maxPendingExtractions bounds the extraction debt, past which the walk goes opaque.
const maxPendingExtractions = 2

func raiseLevel(level viewLevel, steps int) viewLevel {
	raised := int(level) + steps
	if raised > int(levelDeep) {
		return levelDeep
	}
	return viewLevel(raised)
}

func levelForSteps(steps int) viewLevel {
	return raiseLevel(levelWhole, steps)
}

// insertLevel places contents one level in, unless a pending extraction reads them back out.
func insertLevel(level viewLevel, pending int) (viewLevel, int) {
	if pending > 0 {
		return level, pending - 1
	}
	return raiseLevel(level, 1), 0
}

// applyLevel reports imprecise when a deep fact would need its exact count.
func applyLevel(level viewLevel, pending int, fact viewLevel) (viewLevel, int, bool) {
	if fact == levelDeep {
		if pending > 0 {
			return level, pending, false
		}
		return levelDeep, 0, true
	}
	consumed := min(pending, int(fact))
	return raiseLevel(level, int(fact)-consumed), pending - consumed, true
}

func viewDepthForLevel(level viewLevel) ViewDepth {
	if level == levelWhole {
		return DepthWhole
	}
	return DepthElement
}

func (s *aliasSources) markOpaque() {
	s.opaque = true
}

func (s *aliasSources) markShared() {
	s.shared = true
}

func (s *aliasSources) addParam(index int, level viewLevel) {
	if existing, ok := s.params[index]; !ok || level < existing {
		s.params[index] = level
	}
}

func (s *aliasSources) addFreeVar(index int, level viewLevel) {
	if existing, ok := s.freeVars[index]; !ok || level < existing {
		s.freeVars[index] = level
	}
}

func (m *mutationSummary) result(index int) *aliasSources {
	if m.results == nil {
		m.results = map[int]*aliasSources{}
	}
	sources, ok := m.results[index]
	if !ok {
		sources = &aliasSources{params: map[int]viewLevel{}, freeVars: map[int]viewLevel{}}
		m.results[index] = sources
	}
	return sources
}

// weight is monotone: findings only ever appear, lower toward whole, or mark.
func (s *aliasSources) weight() int {
	total := 0
	for _, level := range s.params {
		total += int(levelDeep) + 1 - int(level)
	}
	for _, level := range s.freeVars {
		total += int(levelDeep) + 1 - int(level)
	}
	if s.opaque {
		total++
	}
	if s.shared {
		total++
	}
	return total
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
	for _, sources := range m.results {
		total += sources.weight()
	}
	if m.handOut != nil {
		total += m.handOut.weight()
	}
	for _, buckets := range m.freeVarStores {
		total += buckets.weight()
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
		views:      make(map[*types.Func]FunctionViews),
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

// Views reports the derived fact alone, before ResolveViews layers the rest on.
func (a *MutationAnalysis) Views(obj types.Object) (FunctionViews, bool) {
	if a == nil {
		return FunctionViews{}, false
	}
	fn, ok := obj.(*types.Func)
	if !ok {
		return FunctionViews{}, false
	}
	facts, ok := a.views[fn]
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
		a.verdicts[fn] = facts        // the curated map and the name heuristic decide
		a.views[fn] = FunctionViews{} // the override layer and the worst case decide
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
	a.views[fn] = recordedViews(summary, sig)
}

func recordedViews(summary *mutationSummary, sig *types.Signature) FunctionViews {
	views := FunctionViews{Analyzed: true, Results: make([]ResultView, sig.Results().Len())}
	offset := 0
	if sig.Recv() != nil {
		offset = 1
	}
	paramCount := sig.Params().Len()
	for resultIndex, sources := range summary.results {
		if resultIndex < 0 || resultIndex >= len(views.Results) {
			continue
		}
		view := &views.Results[resultIndex]
		view.Opaque = sources.opaque
		view.Shared = sources.shared
		for index, level := range sources.params {
			if sig.Recv() != nil && index == 0 {
				view.Receiver = joinDepth(view.Receiver, viewDepthForLevel(level))
				continue
			}
			view.setParam(index-offset, paramCount, viewDepthForLevel(level))
		}
	}
	return views
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
			if returned, ok := instruction.(*ssa.Return); ok {
				a.recordViews(body, returned, summary)
				continue
			}
			a.recordHandOuts(body, instruction, summary)
			a.recordCaptureStores(body, instruction, summary)
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
	if common.IsInvoke() {
		if isReceiverMutationAxiom(common.Method) {
			markRoots(fn, common.Value, summary)
		}
		return
	}
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

func isReceiverMutationAxiom(method *types.Func) bool {
	if method == nil || method.Pkg() == nil {
		return false
	}
	return method.Pkg().Path() == "sort" && qualifiedFunctionName(method) == "Interface.Swap"
}

func (a *MutationAnalysis) recordViews(fn *ssa.Function, returned *ssa.Return, summary *mutationSummary) {
	for index, operand := range returned.Results {
		if !containerBearing(operand.Type()) {
			continue
		}
		a.markValueSources(fn, operand, summary.result(index), levelWhole, 0, make(map[sourceVisit]bool))
	}
}

// markValueSources: a copied aggregate shares only through its fields, one level in.
func (a *MutationAnalysis) markValueSources(fn *ssa.Function, value ssa.Value, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) {
	if value != nil && isAggregateValue(value.Type()) {
		level, pending = insertLevel(level, pending)
	}
	a.markSources(fn, value, sources, level, pending, seen)
}

func isAggregateValue(t types.Type) bool {
	if parameter, ok := t.(*types.TypeParam); ok {
		core := coreType(parameter)
		if core == nil {
			return false
		}
		t = core
	}
	switch t.Underlying().(type) {
	case *types.Struct, *types.Array:
		return true
	}
	return false
}

// sourceVisit keys the visited set, since each state gives different answers.
type sourceVisit struct {
	value   ssa.Value
	level   viewLevel
	pending int
}

// markSources walks back from a returned value to the sources it can share
// storage with. Fail-closed: every value kind is precise, fresh (nil Const),
// shared (Global), a documented blind spot (MultiConvert, dynamic dispatch),
// or opaque by default, never fresh.
func (a *MutationAnalysis) markSources(fn *ssa.Function, value ssa.Value, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) {
	visit := sourceVisit{value, level, pending}
	if value == nil || seen[visit] || !containerBearing(value.Type()) {
		return
	}
	seen[visit] = true
	descend := func(next ssa.Value, nextLevel viewLevel) {
		a.markSources(fn, next, sources, nextLevel, pending, seen)
	}
	extract := func(next ssa.Value) {
		if pending == maxPendingExtractions {
			sources.markOpaque()
			return
		}
		a.markSources(fn, next, sources, level, pending+1, seen)
	}

	switch typed := value.(type) {
	case *ssa.Parameter:
		if index := parameterIndex(fn, typed); index >= 0 && recordableSource(typed.Type()) {
			sources.addParam(index, level)
		}
	case *ssa.Global:
		sources.markShared()
	case *ssa.FreeVar:
		if !recordableSource(typed.Type()) {
			return
		}
		for index, free := range fn.FreeVars {
			if free == typed {
				sources.addFreeVar(index, level)
				return
			}
		}
	case *ssa.Slice:
		descend(typed.X, level)
	case *ssa.IndexAddr:
		descend(typed.X, level)
	case *ssa.FieldAddr:
		descend(typed.X, level)
	case *ssa.Field:
		extract(typed.X)
	case *ssa.Index:
		extract(typed.X)
	case *ssa.ChangeType:
		descend(typed.X, level)
	case *ssa.Convert:
		// A string conversion allocates, so the result is storage of its own.
		if !convertCopies(typed.X.Type(), typed.Type()) {
			descend(typed.X, level)
		}
	case *ssa.SliceToArrayPointer:
		descend(typed.X, level)
	case *ssa.Lookup:
		if typed.CommaOk {
			return
		}
		step := keyStep(typed.Index)
		if !a.descendLocalContents(fn, typed.X, &step, sources, level, pending, seen) {
			extract(typed.X)
		}
	case *ssa.MakeInterface:
		// Boxing copies the value.
		a.markValueSources(fn, typed.X, sources, level, pending, seen)
	case *ssa.ChangeInterface:
		descend(typed.X, level)
	case *ssa.TypeAssert:
		descend(typed.X, level)
	case *ssa.Extract:
		switch tuple := typed.Tuple.(type) {
		case *ssa.TypeAssert:
			if typed.Index == 0 {
				descend(tuple.X, level)
			}
		case *ssa.Lookup:
			if typed.Index != 0 {
				return
			}
			step := keyStep(tuple.Index)
			if !a.descendLocalContents(fn, tuple.X, &step, sources, level, pending, seen) {
				extract(tuple.X)
			}
		case *ssa.Next:
			rang, ok := tuple.Iter.(*ssa.Range)
			if !ok {
				return
			}
			// a bearing map key is outside the fact, so opaque rather than fresh
			if typed.Index == 1 {
				sources.markOpaque()
				return
			}
			if typed.Index != 2 {
				return
			}
			step := unknownStep
			if !a.descendLocalContents(fn, rang.X, &step, sources, level, pending, seen) {
				extract(rang.X)
			}
		case *ssa.Call:
			a.resumeResultWalk(fn, tuple, typed.Index, sources, level, pending, seen)
		default:
			sources.markOpaque()
		}
	case *ssa.Phi:
		for _, edge := range typed.Edges {
			descend(edge, level)
		}
	case *ssa.UnOp:
		// only a load preserves storage
		if typed.Op != token.MUL {
			sources.markOpaque()
			return
		}
		// a local read resolves to what was stored, not a step into a container
		if a.descendLocalContents(fn, typed.X, nil, sources, level, pending, seen) {
			return
		}
		if _, ok := typed.X.(*ssa.FreeVar); ok {
			descend(typed.X, level)
			return
		}
		extract(typed.X)
	case *ssa.Alloc:
		a.descendContents(fn, typed, sources, level, pending, seen)
	case *ssa.MakeSlice, *ssa.MakeMap:
		a.descendContents(fn, value, sources, level, pending, seen)
	case *ssa.MakeClosure:
		// an iterator holds its captures one level inside itself
		inside, remaining := insertLevel(level, pending)
		for _, binding := range typed.Bindings {
			a.descendCell(fn, binding, sources, inside, remaining, seen)
		}
		if target, ok := typed.Fn.(*ssa.Function); ok {
			a.consumeFunctionValue(target, sources)
		} else {
			sources.markOpaque()
		}
	case *ssa.Function:
		a.consumeFunctionValue(typed, sources)
	case *ssa.Call:
		common := typed.Common()
		if builtin, ok := common.Value.(*ssa.Builtin); ok {
			// every other bearing-producing builtin is outside the fact
			if builtin.Name() != "append" {
				sources.markOpaque()
				return
			}
			if len(common.Args) == 0 {
				return
			}
			// only zero capacity guarantees fresh storage, since a full
			// append hands its argument back when it appends nothing
			if !isZeroCapacity(common.Args[0]) {
				descend(common.Args[0], level)
			}
			inside, remaining := insertLevel(level, pending)
			for _, added := range common.Args[1:] {
				root, _ := storagePath(added)
				switch root.(type) {
				case *ssa.Alloc, *ssa.MakeSlice, *ssa.MakeMap:
				default:
					if spreadCarriesStorage(added.Type()) {
						a.markSources(fn, added, sources, inside, remaining, seen)
					}
				}
				values, helperWritten, sharesGlobal := a.containedValues(root)
				if helperWritten {
					sources.markOpaque()
				}
				if sharesGlobal {
					sources.markShared()
				}
				for _, stored := range values {
					a.markValueSources(fn, stored.value, sources, inside, remaining, seen)
				}
			}
			return
		}
		a.resumeResultWalk(fn, typed, 0, sources, level, pending, seen)
	case *ssa.Const:
		// nil, so fresh
	case *ssa.MultiConvert:
		// the documented blind spot
	default:
		sources.markOpaque()
	}
}

// consumeFunctionValue: a body can expose storage beyond its captures.
func (a *MutationAnalysis) consumeFunctionValue(target *ssa.Function, sources *aliasSources) {
	if functionBody(target) == nil {
		sources.markOpaque()
		return
	}
	inner := a.summarize(target)
	consume := func(facts *aliasSources) {
		if facts == nil {
			return
		}
		if facts.opaque {
			sources.markOpaque()
		}
		if facts.shared {
			sources.markShared()
		}
	}
	for _, facts := range inner.results {
		consume(facts)
	}
	consume(inner.handOut)
}

// recordCaptureStores goes opaque for routes the buckets cannot account for.
func (a *MutationAnalysis) recordCaptureStores(fn *ssa.Function, instruction ssa.Instruction, summary *mutationSummary) {
	bucketsFor := func(address ssa.Value) (*captureStores, captureDestKind) {
		root, kind := captureDestination(address)
		if root == nil {
			return nil, kind
		}
		for index, free := range fn.FreeVars {
			if free == root {
				return summary.captureStoresFor(index), kind
			}
		}
		return nil, kind
	}
	record := func(buckets *captureStores, kind captureDestKind, value ssa.Value) {
		switch kind {
		case destRebound:
			a.markValueSources(fn, value, buckets.reboundSources(), levelWhole, 0, make(map[sourceVisit]bool))
		case destElement:
			a.markValueSources(fn, value, buckets.elementSources(), levelWhole, 0, make(map[sourceVisit]bool))
		default:
			buckets.elementSources().markOpaque()
		}
	}
	deepen := func(kind captureDestKind) captureDestKind {
		if kind == destRebound {
			return destElement
		}
		return destOpaque
	}
	switch typed := instruction.(type) {
	case *ssa.Store:
		if buckets, kind := bucketsFor(typed.Addr); buckets != nil {
			record(buckets, kind, typed.Val)
		}
	case *ssa.MapUpdate:
		if buckets, kind := bucketsFor(typed.Map); buckets != nil {
			record(buckets, deepen(kind), typed.Value)
		}
	case *ssa.MakeClosure:
		// a nested closure writes out of the buckets' sight
		target, ok := typed.Fn.(*ssa.Function)
		if !ok {
			return
		}
		inner := a.summarize(target)
		for index, binding := range typed.Bindings {
			if _, written := inner.freeVars[index]; !written {
				continue
			}
			if buckets, _ := bucketsFor(binding); buckets != nil {
				buckets.elementSources().markOpaque()
			}
		}
	case ssa.CallInstruction:
		common := typed.Common()
		if builtin, ok := common.Value.(*ssa.Builtin); ok {
			if builtin.Name() == "copy" && len(common.Args) == 2 {
				if buckets, kind := bucketsFor(common.Args[0]); buckets != nil &&
					spreadCarriesStorage(common.Args[1].Type()) {
					if kind == destRebound {
						a.markSources(fn, common.Args[1], buckets.elementSources(), levelWhole, 0, make(map[sourceVisit]bool))
					} else {
						buckets.elementSources().markOpaque()
					}
				}
			}
			return
		}
		for _, argument := range common.Args {
			if buckets, _ := bucketsFor(argument); buckets != nil && a.calleeWritesArgument(common, argument) {
				buckets.elementSources().markOpaque()
			}
		}
	}
}

// captureDestKind says where inside a capture's storage a write lands.
type captureDestKind uint8

const (
	destRebound captureDestKind = iota
	destElement
	destOpaque
)

func captureDestination(address ssa.Value) (*ssa.FreeVar, captureDestKind) {
	indexSteps := 0
	irregular := false
	for {
		switch typed := address.(type) {
		case *ssa.FreeVar:
			switch {
			case irregular || indexSteps > 1:
				return typed, destOpaque
			case indexSteps == 1:
				return typed, destElement
			default:
				return typed, destRebound
			}
		case *ssa.FieldAddr:
			irregular = true
			address = typed.X
		case *ssa.IndexAddr:
			indexSteps++
			address = typed.X
		case *ssa.Slice:
			if !sliceKeepsIndexes(typed) {
				irregular = true
			}
			address = typed.X
		case *ssa.ChangeType:
			address = typed.X
		case *ssa.SliceToArrayPointer:
			address = typed.X
		case *ssa.UnOp:
			if typed.Op != token.MUL {
				return nil, destOpaque
			}
			address = typed.X
		default:
			return nil, destOpaque
		}
	}
}

// recordHandOuts: a bodyless callee holding a function value could call it with anything.
func (a *MutationAnalysis) recordHandOuts(fn *ssa.Function, instruction ssa.Instruction, summary *mutationSummary) {
	call, ok := instruction.(ssa.CallInstruction)
	if !ok {
		return
	}
	common := call.Common()
	if _, ok := common.Value.(*ssa.Builtin); ok {
		return
	}
	if callee := common.StaticCallee(); callee != nil && !common.IsInvoke() {
		if functionBody(callee) == nil {
			for _, argument := range common.Args {
				if _, isFunc := argument.Type().Underlying().(*types.Signature); isFunc {
					summary.handOutSources().markOpaque()
					return
				}
			}
			return
		}
		a.propagateHandOuts(fn, common, callee, summary)
		return
	}
	for _, argument := range common.Args {
		if containerBearing(argument.Type()) {
			a.markValueSources(fn, argument, summary.handOutSources(), levelWhole, 0, make(map[sourceVisit]bool))
		}
	}
}

func (a *MutationAnalysis) propagateHandOuts(fn *ssa.Function, common *ssa.CallCommon, callee *ssa.Function, summary *mutationSummary) {
	inner := a.summarize(callee)
	if inner.handOut == nil {
		return
	}
	target := summary.handOutSources()
	if inner.handOut.opaque {
		target.markOpaque()
	}
	if inner.handOut.shared {
		target.markShared()
	}
	for index, factLevel := range inner.handOut.params {
		if index < len(common.Args) {
			a.markSources(fn, common.Args[index], target, factLevel, 0, make(map[sourceVisit]bool))
		}
	}
	if closure, ok := common.Value.(*ssa.MakeClosure); ok {
		for index, factLevel := range inner.handOut.freeVars {
			if index < len(closure.Bindings) {
				a.descendCell(fn, closure.Bindings[index], target, factLevel, 0, make(map[sourceVisit]bool))
			}
		}
	}
}

func (a *MutationAnalysis) descendContents(fn *ssa.Function, root ssa.Value, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) {
	inside, remaining := insertLevel(level, pending)
	values, helperWritten, sharesGlobal := a.containedValues(root)
	if helperWritten {
		sources.markOpaque()
	}
	if sharesGlobal {
		sources.markShared()
	}
	for _, stored := range values {
		a.markValueSources(fn, stored.value, sources, inside, remaining, seen)
	}
}

func (a *MutationAnalysis) resumeResultWalk(fn *ssa.Function, call *ssa.Call, resultIndex int, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) {
	common := call.Common()
	if common.IsInvoke() {
		return
	}
	if _, ok := common.Value.(*ssa.Builtin); ok {
		return
	}
	callee := common.StaticCallee()
	if callee == nil {
		return
	}
	// a bodyless callee's summary is empty because nothing was seen
	if functionBody(callee) == nil {
		sources.markOpaque()
		return
	}
	inner := a.summarize(callee)
	facts := inner.results[resultIndex]
	if facts == nil {
		return
	}
	if facts.opaque {
		sources.markOpaque()
	}
	if facts.shared {
		sources.markShared()
	}
	for index, factLevel := range facts.params {
		if index >= len(common.Args) {
			continue
		}
		composed, remaining, precise := applyLevel(level, pending, factLevel)
		if !precise {
			sources.markOpaque()
			continue
		}
		a.markSources(fn, common.Args[index], sources, composed, remaining, seen)
	}
	if closure, ok := common.Value.(*ssa.MakeClosure); ok {
		for index, factLevel := range facts.freeVars {
			if index >= len(closure.Bindings) {
				continue
			}
			composed, remaining, precise := applyLevel(level, pending, factLevel)
			if !precise {
				sources.markOpaque()
				continue
			}
			a.descendCell(fn, closure.Bindings[index], sources, composed, remaining, seen)
		}
	}
}

func (a *MutationAnalysis) descendLocalContents(fn *ssa.Function, address ssa.Value, elementStep *pathStep, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) bool {
	base, loadPath := storagePath(address)
	switch base.(type) {
	case *ssa.Alloc, *ssa.MakeSlice, *ssa.MakeMap:
	default:
		return false
	}
	if elementStep != nil {
		loadPath = append(loadPath, *elementStep)
	}
	stores, helperWritten, sharesGlobal := a.localStores(base)
	if helperWritten {
		sources.markOpaque()
	}
	if sharesGlobal {
		sources.markShared()
	}
	for _, stored := range stores {
		if !pathsCompatible(loadPath, stored.path) {
			continue
		}
		// a store above the loaded slot passes through without the copy clamp
		if len(stored.path) < len(loadPath) {
			a.markSources(fn, stored.value, sources, level, pending, seen)
			continue
		}
		storedLevel, remaining, precise := applyLevel(level, pending, levelForSteps(len(stored.path)-len(loadPath)))
		if !precise {
			sources.markOpaque()
			continue
		}
		a.markValueSources(fn, stored.value, sources, storedLevel, remaining, seen)
	}
	return true
}

func pathsCompatible(loadPath, storePath []pathStep) bool {
	for i := 0; i < len(loadPath) && i < len(storePath); i++ {
		if !stepsCompatible(loadPath[i], storePath[i]) {
			return false
		}
	}
	return true
}

func (a *MutationAnalysis) descendCell(fn *ssa.Function, cell ssa.Value, sources *aliasSources, level viewLevel, pending int, seen map[sourceVisit]bool) {
	if alloc, ok := cell.(*ssa.Alloc); ok {
		values, helperWritten, sharesGlobal := a.containedValues(alloc)
		if helperWritten {
			sources.markOpaque()
		}
		if sharesGlobal {
			sources.markShared()
		}
		for _, stored := range values {
			storedLevel, remaining := level, pending
			if stored.nested {
				storedLevel, remaining = insertLevel(level, pending)
			}
			a.markValueSources(fn, stored.value, sources, storedLevel, remaining, seen)
		}
		return
	}
	a.markSources(fn, cell, sources, level, pending, seen)
}

type containedValue struct {
	value  ssa.Value
	nested bool
}

func (a *MutationAnalysis) containedValues(root ssa.Value) ([]containedValue, bool, bool) {
	stores, helperWritten, sharesGlobal := a.localStores(root)
	out := make([]containedValue, len(stores))
	for i, stored := range stores {
		out[i] = containedValue{stored.value, len(stored.path) > 0}
	}
	return out, helperWritten, sharesGlobal
}

// pathStep is one step of a storage path. Unknown steps match anything.
type pathStep struct {
	field int            // field or constant index, -1 when unknown
	key   constant.Value // constant map key, nil when unknown or not a map step
}

var unknownStep = pathStep{field: -1}

func indexStep(index ssa.Value, shifted bool) pathStep {
	if shifted {
		return unknownStep
	}
	if value, ok := constantIndex(index); ok && value >= 0 && int64(int(value)) == value {
		return pathStep{field: int(value)}
	}
	return unknownStep
}

func keyStep(key ssa.Value) pathStep {
	if konst, ok := key.(*ssa.Const); ok && konst.Value != nil {
		return pathStep{field: -1, key: konst.Value}
	}
	return unknownStep
}

func stepsCompatible(a, b pathStep) bool {
	if a.key != nil && b.key != nil {
		return a.key.Kind() == b.key.Kind() && constant.Compare(a.key, token.EQL, b.key)
	}
	return a.field == b.field || a.field == -1 || b.field == -1
}

func sliceKeepsIndexes(sliced *ssa.Slice) bool {
	if sliced.Low == nil {
		return true
	}
	low, ok := constantIndex(sliced.Low)
	return ok && low == 0
}

type localStore struct {
	value ssa.Value
	path  []pathStep
}

// localStores collects every store into the given storage root, with
// out-of-sight and shared markers. Fail-closed: every referrer extends the
// graph, is a collected write, is a silent read, or counts as written out
// of sight, including any unlisted use.
func (a *MutationAnalysis) localStores(base ssa.Value) ([]localStore, bool, bool) {
	var out []localStore
	helperWritten := false
	sharesGlobal := false
	seen := map[ssa.Value]bool{}
	var visit func(v ssa.Value, path []pathStep, shifted bool)
	visit = func(v ssa.Value, path []pathStep, shifted bool) {
		if v == nil || seen[v] {
			return
		}
		seen[v] = true
		referrers := v.Referrers()
		if referrers == nil {
			return
		}
		for _, ref := range *referrers {
			switch typed := ref.(type) {
			case *ssa.Store:
				if typed.Addr == v {
					out = append(out, localStore{value: typed.Val, path: path})
					continue
				}
				// a non-local destination is written out of sight
				root, _ := storagePath(typed.Addr)
				switch root.(type) {
				case *ssa.Alloc, *ssa.MakeSlice, *ssa.MakeMap, *ssa.FreeVar:
					visit(typed.Addr, path, true)
				default:
					helperWritten = true
				}
			case *ssa.FieldAddr:
				if typed.X == v {
					visit(typed, append(slices.Clone(path), pathStep{field: typed.Field}), false)
				}
			case *ssa.IndexAddr:
				if typed.X == v {
					visit(typed, append(slices.Clone(path), indexStep(typed.Index, shifted)), false)
				}
			case *ssa.MapUpdate:
				// only the value is content, since keys are outside the fact
				switch {
				case typed.Map == v:
					out = append(out, localStore{value: typed.Value, path: append(slices.Clone(path), keyStep(typed.Key))})
				case typed.Value == v || typed.Key == v:
					helperWritten = true
				}
			case *ssa.Slice:
				if typed.X == v {
					visit(typed, path, shifted || !sliceKeepsIndexes(typed))
				}
			case *ssa.SliceToArrayPointer:
				if typed.X == v {
					visit(typed, path, shifted)
				}
			case *ssa.Phi:
				// merged values keep the content graph, indexes unaligned
				if loadedValueWritable(typed.Type()) {
					visit(typed, path, true)
				}
			case *ssa.UnOp:
				// a load aliases the same content, when it can hold a bearing value
				if typed.Op == token.MUL && typed.X == v && loadedValueWritable(typed.Type()) {
					visit(typed, path, shifted)
				}
			case *ssa.ChangeType:
				if typed.X == v {
					visit(typed, path, shifted)
				}
			case *ssa.MakeInterface, *ssa.ChangeInterface, *ssa.TypeAssert:
				// boxing and unboxing preserve the header
				visit(typed.(ssa.Value), path, shifted)
			case *ssa.Extract:
				// reachable only through a comma-ok unbox
				if typed.Index == 0 {
					visit(typed, path, shifted)
				}
			case *ssa.MakeClosure:
				a.collectClosureStores(typed, v, path, &out, &helperWritten, &sharesGlobal)
			case *ssa.Call:
				a.collectCallStores(typed, v, path, shifted, &out, &helperWritten, visit)
			case *ssa.Defer:
				if a.calleeWritesArgument(typed.Common(), v) {
					helperWritten = true
				}
			case *ssa.Go:
				if a.calleeWritesArgument(typed.Common(), v) {
					helperWritten = true
				}
			case *ssa.Convert:
				if isUnsafePointerType(typed.Type()) {
					helperWritten = true
				}
			case *ssa.Lookup, *ssa.Range, *ssa.Field, *ssa.Index,
				*ssa.BinOp, *ssa.Return, *ssa.DebugRef:
			default:
				helperWritten = true
			}
		}
	}
	visit(base, nil, false)
	return out, helperWritten, sharesGlobal
}

func (a *MutationAnalysis) collectCallStores(call *ssa.Call, v ssa.Value, path []pathStep, shifted bool, out *[]localStore, helperWritten *bool, visit func(ssa.Value, []pathStep, bool)) {
	common := call.Common()
	builtin, ok := common.Value.(*ssa.Builtin)
	if !ok {
		if a.calleeWritesArgument(common, v) {
			*helperWritten = true
		}
		return
	}
	// copy places the source's element values, like a spread append
	if builtin.Name() == "copy" && len(common.Args) == 2 && common.Args[0] == v {
		if spreadCarriesStorage(common.Args[1].Type()) {
			*out = append(*out, localStore{value: common.Args[1], path: append(slices.Clone(path), unknownStep)})
		}
		return
	}
	if builtin.Name() != "append" || len(common.Args) == 0 || common.Args[0] != v {
		return
	}
	// appended elements land at positions that never align with the storage
	if !appendMustReallocate(common.Args[0]) {
		for _, added := range common.Args[1:] {
			root, _ := storagePath(added)
			visit(root, path, true)
			switch root.(type) {
			case *ssa.Alloc, *ssa.MakeSlice, *ssa.MakeMap:
			default:
				if spreadCarriesStorage(added.Type()) {
					*out = append(*out, localStore{value: added, path: append(slices.Clone(path), unknownStep)})
				}
			}
		}
	}
	if !isZeroCapacity(common.Args[0]) {
		visit(call, path, shifted)
	}
}

func isUnsafePointerType(t types.Type) bool {
	basic, ok := t.Underlying().(*types.Basic)
	return ok && basic.Kind() == types.UnsafePointer
}

func loadedValueWritable(t types.Type) bool {
	if parameter, ok := t.(*types.TypeParam); ok {
		core := coreType(parameter)
		if core == nil {
			return true
		}
		t = core
	}
	switch underlying := t.Underlying().(type) {
	case *types.Slice:
		return recordableSource(underlying.Elem())
	case *types.Map:
		return recordableSource(underlying.Elem())
	case *types.Pointer:
		return recordableSource(underlying.Elem())
	case *types.Interface:
		return true
	}
	return false
}

// collectClosureStores resolves stored parameters through direct invocations
// only, since values arriving through a function value belong to the
// dynamic-dispatch blind spot.
func (a *MutationAnalysis) collectClosureStores(closure *ssa.MakeClosure, v ssa.Value, path []pathStep, out *[]localStore, opaque, shared *bool) {
	target, ok := closure.Fn.(*ssa.Function)
	if !ok {
		*opaque = true
		return
	}
	if functionBody(target) == nil {
		*opaque = true
		return
	}
	inner := a.summarize(target)
	apply := func(facts *aliasSources, entryPath []pathStep) {
		if facts == nil {
			return
		}
		if facts.opaque {
			*opaque = true
		}
		if facts.shared {
			*shared = true
		}
		// the path encodes a fact's level as unknown steps
		leveled := func(level viewLevel) []pathStep {
			path := slices.Clone(entryPath)
			for range int(level) {
				path = append(path, unknownStep)
			}
			return path
		}
		for other, level := range facts.freeVars {
			if other >= len(closure.Bindings) {
				continue
			}
			cell := closure.Bindings[other]
			if otherCell, ok := cell.(*ssa.Alloc); ok {
				for _, held := range storesInto(otherCell) {
					*out = append(*out, localStore{value: held, path: leveled(level)})
				}
				continue
			}
			*out = append(*out, localStore{value: cell, path: leveled(level)})
		}
		if len(facts.params) == 0 {
			return
		}
		referrers := closure.Referrers()
		if referrers == nil {
			return
		}
		for _, ref := range *referrers {
			call, ok := ref.(ssa.CallInstruction)
			if !ok || call.Common().Value != closure {
				continue
			}
			for paramIndex, level := range facts.params {
				if paramIndex < len(call.Common().Args) {
					*out = append(*out, localStore{value: call.Common().Args[paramIndex], path: leveled(level)})
				}
			}
		}
	}
	for index, binding := range closure.Bindings {
		if binding != v {
			continue
		}
		buckets := inner.freeVarStores[index]
		if buckets == nil {
			if _, written := inner.freeVars[index]; written {
				*opaque = true
			}
			continue
		}
		apply(buckets.rebound, path)
		apply(buckets.elements, append(slices.Clone(path), unknownStep))
	}
}

// calleeWritesArgument counts a bodyless callee as a possible writer.
func (a *MutationAnalysis) calleeWritesArgument(common *ssa.CallCommon, v ssa.Value) bool {
	if common.IsInvoke() {
		return false
	}
	callee := common.StaticCallee()
	if callee == nil {
		return false
	}
	bodyless := functionBody(callee) == nil
	inner := a.summarize(callee)
	for i, argument := range common.Args {
		if argument != v {
			continue
		}
		if bodyless {
			return true
		}
		if _, written := inner.params[i]; written {
			return true
		}
	}
	return false
}

func storagePath(value ssa.Value) (ssa.Value, []pathStep) {
	var reversed []pathStep
	for {
		switch typed := value.(type) {
		case *ssa.FieldAddr:
			reversed = append(reversed, pathStep{field: typed.Field})
			value = typed.X
		case *ssa.IndexAddr:
			reversed = append(reversed, indexStep(typed.Index, false))
			value = typed.X
		case *ssa.Slice:
			if !sliceKeepsIndexes(typed) && len(reversed) > 0 {
				reversed[len(reversed)-1] = unknownStep
			}
			value = typed.X
		case *ssa.ChangeType:
			value = typed.X
		default:
			slices.Reverse(reversed)
			return value, reversed
		}
	}
}

func spreadCarriesStorage(t types.Type) bool {
	if parameter, ok := t.(*types.TypeParam); ok {
		core := coreType(parameter)
		if core == nil {
			return true
		}
		t = core
	}
	sliceType, ok := t.Underlying().(*types.Slice)
	if !ok {
		return false
	}
	return recordableSource(sliceType.Elem())
}

// containerBearing reports whether this type can expose a writable container.
func containerBearing(t types.Type) bool {
	return exposesContainer(t, true, make(map[types.Type]bool))
}

// recordableSource: a function value never records, only its captures share storage.
func recordableSource(t types.Type) bool {
	return exposesContainer(t, false, make(map[types.Type]bool))
}

func exposesContainer(t types.Type, funcsBear bool, seen map[types.Type]bool) bool {
	if seen[t] {
		return false
	}
	seen[t] = true
	if parameter, ok := t.(*types.TypeParam); ok {
		core := coreType(parameter)
		if core == nil {
			return true // several shapes admitted: assume one that bears
		}
		t = core
	}
	switch underlying := t.Underlying().(type) {
	case *types.Slice, *types.Map:
		return true
	case *types.Interface:
		return true // may box any container
	case *types.Signature:
		return funcsBear
	case *types.Pointer:
		return exposesContainer(underlying.Elem(), funcsBear, seen)
	case *types.Array:
		return exposesContainer(underlying.Elem(), funcsBear, seen)
	case *types.Struct:
		for i := range underlying.NumFields() {
			if exposesContainer(underlying.Field(i).Type(), funcsBear, seen) {
				return true
			}
		}
		return false
	}
	return false
}

// ResolveViews layers overrides over the derived fact. Overrides only add,
// unresolved results take the worst case unless a clean override completes
// them, and invalid entries come back for the caller to surface.
func ResolveViews(derived FunctionViews, overrides []string, hasOverride bool, sig *types.Signature) (FunctionViews, []string) {
	resolved := FunctionViews{Analyzed: derived.Analyzed, Results: make([]ResultView, sig.Results().Len())}
	for i := range derived.Results {
		if i < len(resolved.Results) {
			resolved.Results[i] = derived.Results[i].clone()
		}
	}
	var invalid []string
	for _, entry := range overrides {
		if !applyViewOverride(&resolved, entry, sig) {
			invalid = append(invalid, entry)
		}
	}
	overridesUsable := hasOverride && len(invalid) == 0
	for i := range resolved.Results {
		if !containerBearing(sig.Results().At(i).Type()) {
			continue
		}
		if !resolved.Results[i].Opaque && derived.Analyzed {
			continue
		}
		if overridesUsable {
			resolved.Results[i].Opaque = false
			continue
		}
		resolved.Results[i].Opaque = true
		applyWorstCaseView(&resolved.Results[i], sig)
	}
	return resolved, invalid
}

// applyViewOverride parses "<result>:<param>", brackets for element depth,
// "recv" for the receiver, reporting whether the entry applied cleanly.
func applyViewOverride(views *FunctionViews, entry string, sig *types.Signature) bool {
	resultPart, source, ok := strings.Cut(entry, ":")
	if !ok {
		return false
	}
	resultIndex, err := strconv.Atoi(resultPart)
	if err != nil || resultIndex < 0 || resultIndex >= len(views.Results) {
		return false
	}
	depth := DepthWhole
	if inner, bracketed := strings.CutPrefix(source, "["); bracketed {
		inner, bracketed = strings.CutSuffix(inner, "]")
		if !bracketed {
			return false
		}
		source, depth = inner, DepthElement
	}
	view := &views.Results[resultIndex]
	if source == "recv" {
		if sig.Recv() == nil {
			return false
		}
		view.Receiver = joinDepth(view.Receiver, depth)
		return true
	}
	params := sig.Params()
	for i := 0; i < params.Len(); i++ {
		if params.At(i).Name() == source && recordableSource(params.At(i).Type()) {
			view.setParam(i, params.Len(), depth)
			return true
		}
	}
	return false
}

// applyWorstCaseView aliases every container-exposing argument at whole depth.
func applyWorstCaseView(view *ResultView, sig *types.Signature) {
	params := sig.Params()
	for j := 0; j < params.Len(); j++ {
		if recordableSource(params.At(j).Type()) {
			view.setParam(j, params.Len(), DepthWhole)
		}
	}
	if receiver := sig.Recv(); receiver != nil && recordableSource(receiver.Type()) {
		view.Receiver = joinDepth(view.Receiver, DepthWhole)
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
