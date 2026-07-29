package convert

import (
	"os"
	"path/filepath"

	"go/types"
	"testing"

	"golang.org/x/tools/go/packages"
)

func analyzeSource(t *testing.T, source string) (*MutationAnalysis, *packages.Package) {
	t.Helper()
	dir := t.TempDir()
	write := func(name, content string) {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("go.mod", "module probe\n\ngo 1.25\n")
	write("probe.go", "package probe\n\n"+source)

	cfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(cfg, "probe")
	if err != nil {
		t.Fatal(err)
	}
	if len(pkgs) != 1 || len(pkgs[0].Errors) > 0 {
		t.Fatalf("load failed: %v", pkgs)
	}
	nilness := NewNilnessAnalysis(pkgs, nil)
	if nilness == nil {
		t.Fatal("SSA build failed")
	}
	analysis := NewMutationAnalysis(nilness, pkgs)
	if analysis == nil {
		t.Fatal("mutation analysis unavailable")
	}
	return analysis, pkgs[0]
}

func mutatedParams(t *testing.T, analysis *MutationAnalysis, pkg *packages.Package, name string) []string {
	t.Helper()
	obj, _ := pkg.Types.Scope().Lookup(name).(*types.Func)
	if obj == nil {
		t.Fatalf("no function %q", name)
	}
	facts, ok := analysis.Function(obj)
	if !ok {
		t.Fatalf("no verdict for %q", name)
	}
	sig := obj.Type().(*types.Signature)
	var out []string
	for i := 0; i < sig.Params().Len(); i++ {
		if facts.Mutates(i) {
			out = append(out, sig.Params().At(i).Name())
		}
	}
	return out
}

func assertMutates(t *testing.T, got []string, want ...string) {
	t.Helper()
	if len(got) != len(want) {
		t.Fatalf("mutated params: got %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("mutated params: got %v, want %v", got, want)
		}
	}
}

func TestMutationDetectsDirectWrites(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func ElementWrite(s []int)          { s[0] = 1 }
func MapWrite(m map[string]int)     { m["k"] = 1 }
func MapDelete(m map[string]int)    { delete(m, "k") }
func SliceClear(s []int)            { clear(s) }
func CopyInto(dst, src []int)       { copy(dst, src) }
func ReadOnly(s []int) int          { return s[0] }
func Reassign(s []int) []int        { s = nil; return s }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "ElementWrite"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "MapWrite"), "m")
	assertMutates(t, mutatedParams(t, analysis, pkg, "MapDelete"), "m")
	assertMutates(t, mutatedParams(t, analysis, pkg, "SliceClear"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "CopyInto"), "dst")
	assertMutates(t, mutatedParams(t, analysis, pkg, "ReadOnly"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "Reassign"))
}

// `slices.Sort` stores nothing itself and reaches its write several frames
// down, so depth matters.
func TestMutationPropagatesThroughCallees(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func deep3(s []int)      { s[0] = 1 }
func deep2(s []int)      { deep3(s) }
func deep1(s []int)      { deep2(s) }
func Shallow(s []int)    { deep1(s) }
func Sub(s []int)        { deep1(s[1:]) }
func NotPassed(s []int)  { deep1(nil) }
func Recursive(s []int)  { if len(s) > 0 { Recursive(s[1:]) }; s[0] = 1 }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Shallow"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "Sub"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "NotPassed"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "Recursive"), "s")
}

// A closure may run wherever it is handed to, as `maps.Insert` does.
func TestMutationSeesClosureWrites(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func run(f func()) { f() }

func ThroughClosure(s []int)  { run(func() { s[0] = 1 }) }
func EscapingClosure(s []int) func() { return func() { s[0] = 1 } }
func ReadingClosure(s []int)  { run(func() { _ = s[0] }) }
func ReassignInClosure(s []int) { run(func() { s = nil }) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "ThroughClosure"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "EscapingClosure"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "ReadingClosure"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "ReassignInClosure"))
}

// Without these the whole `slices` package is invisible.
func TestMutationSeesGenericSliceParams(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func GenericWrite[S ~[]E, E any](s S)     { var zero E; s[0] = zero }
func GenericRead[S ~[]E, E any](s S) int  { return len(s) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "GenericWrite"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "GenericRead"))
}

// Both halves matter: the write catches `s = append(s[:0], v)`, the
// provenance catches `slices.Delete`.
func TestMutationFollowsAppend(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Reset(s []int)                 { s = append(s[:0], 42); _ = s }
func AppendAndReturn(s []int) []int { return append(s, 1) }

func DeleteShape(s []int, i, j int) []int {
	oldlen := len(s)
	s = append(s[:i], s[j:]...)
	clear(s[len(s):oldlen])
	return s
}

func FreshBuilder(s []int) []int { out := []int{}; for _, v := range s { out = append(out, v) }; return out }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Reset"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "AppendAndReturn"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "DeleteShape"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "FreshBuilder"))
}

// The shapes `slices.Clone` and `slices.Grow` use to force a fresh allocation.
func TestMutationSkipsReallocatingAppend(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func CloneShape(s []int) []int { return append(s[:0:0], s...) }

func GrowShape(s []int, n int) []int {
	if n -= cap(s) - len(s); n > 0 { s = append(s[:cap(s)], make([]int, n)...)[:len(s)] }
	return s
}
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "CloneShape"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "GrowShape"))
}

// A filled-to-capacity append hands its argument straight back when it appends
// nothing, so only the zero-capacity form breaks the chain.
func TestMutationBreaksProvenanceOnlyForZeroCapacityAppend(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func CloneThenClear(s []int) { out := append(s[:0:0], s...); clear(out) }
func FullThenClear(s []int, xs []int) { out := append(s[:cap(s)], xs...); clear(out) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "CloneThenClear"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "FullThenClear"), "s")
}

// `cycA` writes after its recursive call, so `cycB` reads an empty in-progress
// summary and must not keep it.
func TestMutationSettlesMutualRecursion(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func cycA(s []int, n int) { if n > 0 { cycB(s, n-1) }; s[0] = 1 }
func cycB(s []int, n int) { if n > 0 { cycA(s, n-1) } }

func EntryA(s []int) { cycA(s, 1) }
func EntryB(s []int) { cycB(s, 1) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "EntryA"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "EntryB"), "s")
}

// Repair walks backwards one link per pass in the worst ordering, so a settle
// that stopped early would leave entries disagreeing.
func TestMutationSettlesLongCycleFromEveryEntry(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func cyc1(s []int, n int)  { if n > 0 { cyc2(s, n-1) }; s[0] = 1 }
func cyc2(s []int, n int)  { if n > 0 { cyc3(s, n-1) } }
func cyc3(s []int, n int)  { if n > 0 { cyc4(s, n-1) } }
func cyc4(s []int, n int)  { if n > 0 { cyc5(s, n-1) } }
func cyc5(s []int, n int)  { if n > 0 { cyc6(s, n-1) } }
func cyc6(s []int, n int)  { if n > 0 { cyc7(s, n-1) } }
func cyc7(s []int, n int)  { if n > 0 { cyc8(s, n-1) } }
func cyc8(s []int, n int)  { if n > 0 { cyc9(s, n-1) } }
func cyc9(s []int, n int)  { if n > 0 { cyc10(s, n-1) } }
func cyc10(s []int, n int) { if n > 0 { cyc11(s, n-1) } }
func cyc11(s []int, n int) { if n > 0 { cyc12(s, n-1) } }
func cyc12(s []int, n int) { if n > 0 { cyc1(s, n-1) } }

func Entry2(s []int)  { cyc2(s, 1) }
func Entry7(s []int)  { cyc7(s, 1) }
func Entry12(s []int) { cyc12(s, 1) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Entry2"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "Entry7"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "Entry12"), "s")
}

func TestMutationStopsAtIndirectElements(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type box struct{ n int }
type node interface{ node() }
func (b *box) node() {}

func WriteThroughElement(list []*box)  { for _, b := range list { b.n = 1 } }
func WriteThroughUnboxed(list []node)  { for _, e := range list { if b, ok := e.(*box); ok { b.n = 1 } } }
func WriteValueElement(list []box)     { for i := range list { list[i].n = 1 } }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "WriteThroughElement"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "WriteThroughUnboxed"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "WriteValueElement"), "list")
}

// Boxing shares the backing store, so unboxing and writing reaches the caller.
func TestMutationFollowsInterfaceBoxing(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func unbox(v any)        { v.([]int)[0] = 1 }
func unboxCommaOk(v any) { if got, ok := v.([]int); ok { got[0] = 1 } }
func readUnboxed(v any) int { return len(v.([]int)) }

func Boxed(s []int)        { unbox(s) }
func BoxedCommaOk(s []int) { unboxCommaOk(s) }
func BoxedRead(s []int)    { readUnboxed(s) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Boxed"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "BoxedCommaOk"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "BoxedRead"))
}

// A slot is not a container element, so the cutoff turns on provenance rather
// than on the loaded type alone.
func TestMutationFollowsInterfaceThroughVariableSlots(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Captured(s []int) { v := any(s); func() { v.([]int)[0] = 1 }() }
func Local(s []int)    { v := any(s); v.([]int)[0] = 1 }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Captured"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "Local"), "s")
}

// A type parameter reports its constraint as its underlying type, and that
// constraint may itself be named or embed another named one.
func TestMutationResolvesTypeParameterCore(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
import "iter"

type sliceish interface{ ~[]int }
type nested interface{ sliceish }
type withMethod interface{ ~[]int; Len() int }

func AppendSeqShape[S ~[]E, E any](s S, seq iter.Seq[E]) S {
	for v := range seq { s = append(s, v) }
	return s
}

func InlineConstraint[S ~[]int](s S)     { func() { s[0] = 1 }() }
func NamedConstraint[S sliceish](s S)    { func() { s[0] = 1 }() }
func NestedConstraint[S nested](s S)     { func() { s[0] = 1 }() }
func MethodConstraint[S withMethod](s S) { func() { s[0] = 1 }() }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "AppendSeqShape"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "InlineConstraint"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "NamedConstraint"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "NestedConstraint"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "MethodConstraint"), "s")
}

// Each case writes through a loaded element, so the assertion turns on how the
// walk classifies that load rather than on a direct write.
func TestMutationLeavesOpenConstraintsUnresolved(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type sliceish interface{ ~[]int }

func OpenConstraintElement[T any](list []T) {
	for _, e := range list {
		if s, ok := any(e).([]int); ok { s[0] = 1 }
	}
}

func OpenConstraintPointer[T any](list []*T) { for _, p := range list { var zero T; *p = zero } }

func ResolvedConstraint[S sliceish](s S) { func() { s[0] = 1 }() }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "OpenConstraintElement"), "list")
	assertMutates(t, mutatedParams(t, analysis, pkg, "OpenConstraintPointer"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "ResolvedConstraint"), "s")
}

// Reassigning a local says nothing about what it was seeded from.
// `crypto/x509` parses this way, walking a cursor over a copy of its input.
func TestMutationStopsAtLocalReassignment(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func advance(cursor *[]byte) { *cursor = (*cursor)[1:] }

func WalkCursor(der []byte)  { cursor := der; advance(&cursor) }
func WriteCursor(der []byte) { cursor := der; cursor[0] = 1 }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "WalkCursor"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "WriteCursor"), "der")
}

// `writeThrough` reaches its parameter through a load and `replace` does not,
// so a caller passing `&local` has to resume differently for each.
func TestMutationResumesCalleeWalkInTheSameMode(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func writeThrough(p *[]int) { (*p)[0] = 42 }
func replace(p *[]int)      { *p = nil }

func ThroughLocal(s []int) { local := s; writeThrough(&local) }
func ReplaceLocal(s []int) { local := s; replace(&local); _ = local }
func ThroughDirect(s []int) { writeThrough(&s) }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "ThroughLocal"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "ReplaceLocal"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "ThroughDirect"), "s")
}

// A map element read reaches the map's own storage, in all three forms.
func TestMutationFollowsMapLookups(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type node interface{ node() }
type boxed struct{ n int }
func (b *boxed) node() {}

func Direct(m map[string][]int)  { m["x"][0] = 1 }
func CommaOk(m map[string][]int) { if v, ok := m["x"]; ok { v[0] = 1 } }
func Ranged(m map[string][]int)  { for _, s := range m { s[0] = 1 } }
func ReadOnly(m map[string][]int) int { return len(m["x"]) }

func IndirectElement(m map[string]node) {
	for _, e := range m {
		if b, ok := e.(*boxed); ok { b.n = 1 }
	}
}
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "Direct"), "m")
	assertMutates(t, mutatedParams(t, analysis, pkg, "CommaOk"), "m")
	assertMutates(t, mutatedParams(t, analysis, pkg, "Ranged"), "m")
	assertMutates(t, mutatedParams(t, analysis, pkg, "ReadOnly"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "IndirectElement"))
}

// A string conversion allocates. A slice-to-array-pointer one does not.
func TestMutationTracksConversionStorage(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func StringRoundTrip(src []byte) { dst := []byte(string(src)); clear(dst) }
func ToArrayPointer(s []int)     { a := (*[1]int)(s); a[0] = 1 }
func NamedConversion(s []int)    { type ints []int; a := ints(s); a[0] = 1 }
`)
	assertMutates(t, mutatedParams(t, analysis, pkg, "StringRoundTrip"))
	assertMutates(t, mutatedParams(t, analysis, pkg, "ToArrayPointer"), "s")
	assertMutates(t, mutatedParams(t, analysis, pkg, "NamedConversion"), "s")
}

func TestMutationReportsMethodReceiverOffset(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Holder struct{ items []int }

func (h *Holder) Fill(dst []int)     { copy(dst, h.items) }
func (h *Holder) Absorb(src []int)   { h.items = src }
`)
	named := pkg.Types.Scope().Lookup("Holder").(*types.TypeName).Type().(*types.Named)
	verdicts := map[string][]string{}
	for i := 0; i < named.NumMethods(); i++ {
		method := named.Method(i)
		facts, ok := analysis.Function(method)
		if !ok {
			t.Fatalf("no verdict for %q", method.Name())
		}
		sig := method.Type().(*types.Signature)
		var mutated []string
		for j := 0; j < sig.Params().Len(); j++ {
			if facts.Mutates(j) {
				mutated = append(mutated, sig.Params().At(j).Name())
			}
		}
		verdicts[method.Name()] = mutated
	}
	assertMutates(t, verdicts["Fill"], "dst")
	assertMutates(t, verdicts["Absorb"])
}

func TestIsMutableParamCombinesSignals(t *testing.T) {
	cases := []struct {
		name      string
		derived   bool
		curated   []string
		paramName string
		typeStr   string
		funcName  string
		want      bool
	}{
		{"derived only", true, nil, "s", "Slice<int>", "Sort", true},
		{"curated only", false, []string{"buf"}, "buf", "Slice<byte>", "Buffer", true},
		{"heuristic only", false, nil, "p", "Slice<byte>", "Read", true},
		{"curated omits, derived finds", true, []string{"other"}, "s", "Slice<int>", "Sort", true},
		{"no signal", false, nil, "s", "Slice<int>", "Sort", false},
		{"not a reference type", true, []string{"n"}, "n", "int", "Read", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := isMutableParam(c.derived, c.curated, c.paramName, c.typeStr, c.funcName)
			if got != c.want {
				t.Errorf("isMutableParam = %v, want %v", got, c.want)
			}
		})
	}
}
