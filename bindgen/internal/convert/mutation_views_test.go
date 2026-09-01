package convert

import (
	"go/types"
	"sort"
	"testing"

	"golang.org/x/tools/go/packages"
)

func functionViews(t *testing.T, analysis *MutationAnalysis, pkg *packages.Package, name string) (FunctionViews, *types.Signature) {
	t.Helper()
	obj, _ := pkg.Types.Scope().Lookup(name).(*types.Func)
	if obj == nil {
		t.Fatalf("no function %q", name)
	}
	views, ok := analysis.Views(obj)
	if !ok {
		t.Fatalf("no views for %q", name)
	}
	return views, obj.Type().(*types.Signature)
}

func methodViews(t *testing.T, analysis *MutationAnalysis, pkg *packages.Package, typeName, methodName string) (FunctionViews, *types.Signature) {
	t.Helper()
	named := pkg.Types.Scope().Lookup(typeName).(*types.TypeName).Type().(*types.Named)
	for i := 0; i < named.NumMethods(); i++ {
		method := named.Method(i)
		if method.Name() != methodName {
			continue
		}
		views, ok := analysis.Views(method)
		if !ok {
			t.Fatalf("no views for %q.%q", typeName, methodName)
		}
		return views, method.Type().(*types.Signature)
	}
	t.Fatalf("no method %q.%q", typeName, methodName)
	return FunctionViews{}, nil
}

// viewTokens renders "s" for whole depth, "[s]" for element, "recv" for the receiver.
func viewTokens(sig *types.Signature, view ResultView) []string {
	var out []string
	switch view.Receiver {
	case DepthWhole:
		out = append(out, "recv")
	case DepthElement:
		out = append(out, "[recv]")
	}
	for i, depth := range view.Params {
		name := sig.Params().At(i).Name()
		switch depth {
		case DepthWhole:
			out = append(out, name)
		case DepthElement:
			out = append(out, "["+name+"]")
		}
	}
	sort.Strings(out)
	return out
}

func assertOpaque(t *testing.T, views FunctionViews, resultIndex int, want bool) {
	t.Helper()
	if resultIndex >= len(views.Results) {
		t.Fatalf("result %d out of range, have %d results", resultIndex, len(views.Results))
	}
	if views.Results[resultIndex].Opaque != want {
		t.Errorf("result %d: Opaque = %v, want %v", resultIndex, views.Results[resultIndex].Opaque, want)
	}
}

func assertShared(t *testing.T, views FunctionViews, resultIndex int, want bool) {
	t.Helper()
	if resultIndex >= len(views.Results) {
		t.Fatalf("result %d out of range, have %d results", resultIndex, len(views.Results))
	}
	if views.Results[resultIndex].Shared != want {
		t.Errorf("result %d: Shared = %v, want %v", resultIndex, views.Results[resultIndex].Shared, want)
	}
}

func assertResultView(t *testing.T, views FunctionViews, sig *types.Signature, resultIndex int, want ...string) {
	t.Helper()
	if resultIndex >= len(views.Results) {
		t.Fatalf("result %d out of range, have %d results", resultIndex, len(views.Results))
	}
	got := viewTokens(sig, views.Results[resultIndex])
	sort.Strings(want)
	if len(got) != len(want) {
		t.Fatalf("result %d aliases %v, want %v", resultIndex, got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("result %d aliases %v, want %v", resultIndex, got, want)
		}
	}
}

func TestViewsDirectReturns(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Identity(s []int) []int          { return s }
func Tail(s []int) []int              { return s[1:] }
func Choose(a, b []int, c bool) []int { if c { return a }; return b }
func Fresh(n int) []int               { return make([]int, n) }
func CloneShape(s []int) []int        { return append(s[:0:0], s...) }
func StringCopy(s string) []byte      { return []byte(s) }
func Count(s []int) int               { return len(s) }
`)
	views, sig := functionViews(t, analysis, pkg, "Identity")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "Tail")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "Choose")
	assertResultView(t, views, sig, 0, "a", "b")
	views, sig = functionViews(t, analysis, pkg, "Fresh")
	assertResultView(t, views, sig, 0)
	views, sig = functionViews(t, analysis, pkg, "CloneShape")
	assertResultView(t, views, sig, 0)
	views, sig = functionViews(t, analysis, pkg, "StringCopy")
	assertResultView(t, views, sig, 0)
	views, sig = functionViews(t, analysis, pkg, "Count")
	assertResultView(t, views, sig, 0)
}

// A spread whose elements cannot expose a container contributes nothing.
func TestViewsFollowAppend(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func AppendOne(dst []byte, b byte) []byte        { return append(dst, b) }
func AppendSpread(src []byte) []byte             { return append([]byte(nil), src...) }
func AppendRow(rows [][]byte, s []byte) [][]byte { return append(rows, s[1:]) }

func GrowShape(s []int, n int) []int {
	if n -= cap(s) - len(s); n > 0 { s = append(s[:cap(s)], make([]int, n)...)[:len(s)] }
	return s
}

func FullAppend(s, xs []int) []int { return append(s[:cap(s)], xs...) }

func AppendSpreadRows(src [][]byte) [][]byte { return append([][]byte(nil), src...) }

func writeDigits(dst []byte) int { for i := range dst { dst[i] = '0' }; return 0 }

func AppendHelperWritten(dst []byte) []byte {
	var a [8]byte
	j := writeDigits(a[:])
	return append(dst, a[j:]...)
}
`)
	views, sig := functionViews(t, analysis, pkg, "AppendOne")
	assertResultView(t, views, sig, 0, "dst")
	views, sig = functionViews(t, analysis, pkg, "AppendSpread")
	assertResultView(t, views, sig, 0)
	views, sig = functionViews(t, analysis, pkg, "AppendRow")
	assertResultView(t, views, sig, 0, "rows", "[s]")
	views, sig = functionViews(t, analysis, pkg, "GrowShape")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "FullAppend")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "AppendSpreadRows")
	assertResultView(t, views, sig, 0, "[src]")
	views, sig = functionViews(t, analysis, pkg, "AppendHelperWritten")
	assertResultView(t, views, sig, 0, "dst")
	assertOpaque(t, views, 0, false)
}

func TestViewsReportEachResult(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func CutShape(s, sep []byte) ([]byte, []byte, bool) {
	if len(sep) > 0 { return s[:1], s[1:], true }
	return s, nil, false
}
`)
	views, sig := functionViews(t, analysis, pkg, "CutShape")
	assertResultView(t, views, sig, 0, "s")
	assertResultView(t, views, sig, 1, "s")
	assertResultView(t, views, sig, 2)
}

func TestViewsReportReceiverAliasing(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Buffer struct {
	buf []byte
	off int
}

func (b *Buffer) Bytes() []byte { return b.buf[b.off:] }
func (b *Buffer) Len() int      { return len(b.buf) }

type Header map[string][]string

func (h Header) Values(key string) []string { return h[key] }
`)
	views, sig := methodViews(t, analysis, pkg, "Buffer", "Bytes")
	assertResultView(t, views, sig, 0, "recv")
	views, sig = methodViews(t, analysis, pkg, "Buffer", "Len")
	assertResultView(t, views, sig, 0)
	views, sig = methodViews(t, analysis, pkg, "Header", "Values")
	assertResultView(t, views, sig, 0, "recv")
}

// A fresh container holding parameter-derived values reports element depth.
func TestViewsReportElementDepth(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Buffer struct { buf []byte }

func SplitShape(s []byte, n int) [][]byte {
	out := make([][]byte, n)
	for i := 0; i < n; i++ { out[i] = s[i : i+1 : i+1] }
	return out
}

func WrapShape(buf []byte) *Buffer   { return &Buffer{buf: buf} }
func LiteralShape(s []byte) [][]byte { return [][]byte{s} }

func MapShape(s []int) map[string][]int {
	out := map[string][]int{}
	out["k"] = s
	return out
}
`)
	views, sig := functionViews(t, analysis, pkg, "SplitShape")
	assertResultView(t, views, sig, 0, "[s]")
	views, sig = functionViews(t, analysis, pkg, "WrapShape")
	assertResultView(t, views, sig, 0, "[buf]")
	views, sig = functionViews(t, analysis, pkg, "LiteralShape")
	assertResultView(t, views, sig, 0, "[s]")
	views, sig = functionViews(t, analysis, pkg, "MapShape")
	assertResultView(t, views, sig, 0, "[s]")
}

func TestViewsReportIteratorCaptures(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func IteratorShape(s []byte) func(func([]byte) bool) {
	return func(yield func([]byte) bool) {
		for i := 0; i < len(s); i++ {
			if !yield(s[i : i+1]) { return }
		}
	}
}

func CounterShape(n int) func() int {
	return func() int { n++; return n }
}
`)
	views, sig := functionViews(t, analysis, pkg, "IteratorShape")
	assertResultView(t, views, sig, 0, "[s]")
	views, sig = functionViews(t, analysis, pkg, "CounterShape")
	assertResultView(t, views, sig, 0)
}

func TestViewsPropagateThroughCallees(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Buffer struct { buf []byte }

func trim(s []byte) []byte        { return s[1:] }
func pair(s []byte) ([]byte, int) { return s, 0 }
func wrap(buf []byte) *Buffer     { return &Buffer{buf: buf} }

func Trimmed(s []byte) []byte      { return trim(s) }
func Extracted(s []byte) []byte    { v, _ := pair(s); return v }
func Wrapped(s []byte) [][]byte    { return [][]byte{trim(s)} }
func FromWrapper(buf []byte) *Buffer { return wrap(buf) }
`)
	views, sig := functionViews(t, analysis, pkg, "Trimmed")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "Extracted")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "Wrapped")
	assertResultView(t, views, sig, 0, "[s]")
	views, sig = functionViews(t, analysis, pkg, "FromWrapper")
	assertResultView(t, views, sig, 0, "[buf]")
}

func TestViewsFollowElementCopies(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func ElementwiseCopy(src [][]byte) [][]byte {
	out := make([][]byte, len(src))
	for i := range src { out[i] = src[i] }
	return out
}

func CopiedValues(m map[string][]byte) [][]byte {
	out := make([][]byte, 0)
	for _, v := range m { out = append(out, v) }
	return out
}

func FirstValue(m map[string][]byte) []byte { return m["k"] }
`)
	views, sig := functionViews(t, analysis, pkg, "ElementwiseCopy")
	assertResultView(t, views, sig, 0, "[src]")
	views, sig = functionViews(t, analysis, pkg, "CopiedValues")
	assertResultView(t, views, sig, 0, "[m]")
	views, sig = functionViews(t, analysis, pkg, "FirstValue")
	assertResultView(t, views, sig, 0, "m")
}

// Reads carry the stored depth and keep sibling fields apart.
func TestViewsResolveReadsFromLocalStorage(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Wrapper struct { buf, other []byte }

func FieldOfLocal(s []byte) []byte {
	x := &Wrapper{buf: s}
	return x.buf
}

func SiblingFieldOfLocal(s []byte) []byte {
	x := &Wrapper{buf: s}
	return x.other
}

func StructOfLocal(s []byte) Wrapper {
	var tmp Wrapper
	tmp.buf = s
	return tmp
}

func ElementOfLocal(s []byte) []byte {
	out := make([][]byte, 1)
	out[0] = s
	return out[0]
}
`)
	views, sig := functionViews(t, analysis, pkg, "FieldOfLocal")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "SiblingFieldOfLocal")
	assertResultView(t, views, sig, 0)
	views, sig = functionViews(t, analysis, pkg, "StructOfLocal")
	assertResultView(t, views, sig, 0, "[s]")
	views, sig = functionViews(t, analysis, pkg, "ElementOfLocal")
	assertResultView(t, views, sig, 0, "s")
}

// A writing callee stores out of sight, so opaque, while a writing closure
// stays precise for the checklist iterators.
func TestViewsMarkHelperWrittenLocalsOpaque(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func fill(dst [][]byte, v []byte) { dst[0] = v }
func peek(dst [][]byte, v []byte) int { return len(dst) + len(v) }

func FilledByCallee(s []byte) [][]byte {
	out := make([][]byte, 1)
	fill(out, s)
	return out
}

func FilledElement(s []byte) []byte {
	out := make([][]byte, 1)
	fill(out, s)
	return out[0]
}

func FilledByClosure(s []byte) [][]byte {
	out := make([][]byte, 1)
	func() { out[0] = s }()
	return out
}

func FilledByClosureArgument(s []byte) [][]byte {
	out := make([][]byte, 1)
	fill := func(v []byte) { out[0] = v }
	fill(s)
	return out
}

func ReadByCallee(a, b []byte) [][]byte {
	out := [][]byte{a}
	peek(out, b)
	return out
}
`)
	views, _ := functionViews(t, analysis, pkg, "FilledByCallee")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "FilledElement")
	assertOpaque(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "FilledByClosure")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "FilledByClosureArgument")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "ReadByCallee")
	assertOpaque(t, views, 0, false)
	assertResultView(t, views, sig, 0, "[a]")
}

// Byte copies carry no storage.
func TestViewsFollowCopyStoresIntoLocals(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func CopiedRows(src [][]byte) [][]byte {
	out := make([][]byte, len(src))
	copy(out, src)
	return out
}

func CopiedBytes(src []byte) []byte {
	out := make([]byte, len(src))
	copy(out, src)
	return out
}
`)
	views, sig := functionViews(t, analysis, pkg, "CopiedRows")
	assertResultView(t, views, sig, 0, "[src]")
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "CopiedBytes")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
}

// A channel result carries no fact, and a bearing receive goes opaque.
func TestViewsKeepChannelsOutsideTheFact(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func SentInto(s []byte) chan []byte {
	out := make(chan []byte, 1)
	out <- s
	return out
}

func ReceivedBack(s []byte) []byte {
	out := make(chan []byte, 1)
	out <- s
	return <-out
}

func ReceivedCommaOk(s []byte) []byte {
	out := make(chan []byte, 1)
	out <- s
	v, _ := <-out
	return v
}

func ReceivedScalar(numbers chan int) int {
	return <-numbers
}
`)
	views, sig := functionViews(t, analysis, pkg, "SentInto")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "ReceivedBack")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "ReceivedCommaOk")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "ReceivedScalar")
	assertOpaque(t, views, 0, false)
}

// A bearing result built from unsafe memory goes opaque.
func TestViewsKeepUnsafeMemoryOutsideTheFact(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
import "unsafe"

func FromPointer(p *byte, n int) []byte { return unsafe.Slice(p, n) }
func BackingPointer(s []byte) *byte     { return unsafe.SliceData(s) }
func ClipShape(s []byte) []byte         { return unsafe.Slice(unsafe.SliceData(s), len(s)) }
`)
	views, _ := functionViews(t, analysis, pkg, "FromPointer")
	assertOpaque(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "BackingPointer")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "ClipShape")
	assertOpaque(t, views, 0, true)
}

func TestViewsMarkSelectReceivesOpaque(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func SelectReceived(numbers chan int, payloads chan []byte) []byte {
	select {
	case n := <-numbers:
		_ = n
		return nil
	case p := <-payloads:
		return p
	}
}
`)
	views, _ := functionViews(t, analysis, pkg, "SelectReceived")
	assertOpaque(t, views, 0, true)
}

// An offset reslice hides which slot an index lands in.
func TestViewsKeepConstantSlotsApart(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func OtherIndex(a, b []byte) []byte {
	out := make([][]byte, 2)
	out[0] = a
	out[1] = b
	return out[0]
}

func OtherKey(a, b []byte) []byte {
	out := map[string][]byte{}
	out["a"] = a
	out["b"] = b
	return out["a"]
}

func ShiftedIndex(a, b []byte) []byte {
	out := make([][]byte, 3)
	out[0] = a
	rest := out[1:]
	rest[0] = b
	return out[1]
}
`)
	views, sig := functionViews(t, analysis, pkg, "OtherIndex")
	assertResultView(t, views, sig, 0, "a")
	views, sig = functionViews(t, analysis, pkg, "OtherKey")
	assertResultView(t, views, sig, 0, "a")
	views, sig = functionViews(t, analysis, pkg, "ShiftedIndex")
	assertResultView(t, views, sig, 0, "b")
}

// A returned aggregate is a copy, so its sharing sits one level inside it.
func TestViewsClampCopiedAggregates(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Wrapper struct { buf []byte }

func Echo(w Wrapper) Wrapper          { return w }
func EchoField(w Wrapper) []byte      { return w.buf }
func EchoArray(a [2][]byte) [2][]byte { return a }
func Boxed(w Wrapper) any             { return w }

func BoxedCopy(w Wrapper) Wrapper {
	x := any(w)
	return x.(Wrapper)
}

func BoxedFieldOfCopy(w Wrapper) []byte {
	x := any(w)
	return x.(Wrapper).buf
}
`)
	views, sig := functionViews(t, analysis, pkg, "Echo")
	assertResultView(t, views, sig, 0, "[w]")
	views, sig = functionViews(t, analysis, pkg, "EchoField")
	assertResultView(t, views, sig, 0, "w")
	views, sig = functionViews(t, analysis, pkg, "EchoArray")
	assertResultView(t, views, sig, 0, "[a]")
	views, sig = functionViews(t, analysis, pkg, "Boxed")
	assertResultView(t, views, sig, 0, "[w]")
	views, sig = functionViews(t, analysis, pkg, "BoxedCopy")
	assertResultView(t, views, sig, 0, "[w]")
	views, sig = functionViews(t, analysis, pkg, "BoxedFieldOfCopy")
	assertResultView(t, views, sig, 0, "w")
}

func TestViewsReduceDepthWhenExtractingFromCalleeResults(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func wrapRows(s []byte) [][]byte { return [][]byte{s} }

func FirstOfCallee(s []byte) []byte     { return wrapRows(s)[0] }
func RewrappedCallee(s []byte) [][]byte { return [][]byte{wrapRows(s)[0]} }
`)
	views, sig := functionViews(t, analysis, pkg, "FirstOfCallee")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "RewrappedCallee")
	assertResultView(t, views, sig, 0, "[s]")
}

// Extracting from a deep fact needs an exact count that is not kept.
func TestViewsMarkExtractionFromDeepFactsOpaque(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func wrapTwice(s []byte) [][][]byte { return [][][]byte{{s}} }

func DeepIntact(s []byte) [][][]byte     { return wrapTwice(s) }
func FirstOfDeepCallee(s []byte) [][]byte { return wrapTwice(s)[0] }
func SecondOfDeepCallee(s []byte) []byte  { return wrapTwice(s)[0][0] }
`)
	views, sig := functionViews(t, analysis, pkg, "DeepIntact")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "FirstOfDeepCallee")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "SecondOfDeepCallee")
	assertOpaque(t, views, 0, true)
}

// Map keys are outside the fact.
func TestViewsKeepMapKeysOutsideTheFact(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type box struct { data []byte }

func LookedUpValue(k *int, v []byte) []byte {
	out := map[*int][]byte{}
	out[k] = v
	return out[k]
}

func WholeMap(k *int, v []byte) map[*int][]byte {
	out := map[*int][]byte{}
	out[k] = v
	return out
}

func PlainPointerKey(m map[*int][]byte) *int {
	for key := range m {
		return key
	}
	return nil
}

func BearingKey(m map[*box]int) *box {
	for key := range m {
		return key
	}
	return nil
}

func LocalRangedValue(k *int, v []byte) []byte {
	out := map[*int][]byte{k: v}
	for _, value := range out {
		return value
	}
	return nil
}
`)
	views, sig := functionViews(t, analysis, pkg, "LookedUpValue")
	assertResultView(t, views, sig, 0, "v")
	views, sig = functionViews(t, analysis, pkg, "WholeMap")
	assertResultView(t, views, sig, 0, "[v]")
	views, sig = functionViews(t, analysis, pkg, "PlainPointerKey")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "BearingKey")
	assertOpaque(t, views, 0, true)
	views, sig = functionViews(t, analysis, pkg, "LocalRangedValue")
	assertResultView(t, views, sig, 0, "v")
}

// A loaded pointer still designates an object reachable from its container.
func TestViewsFollowContainerBearingPointerElements(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type box struct { data []byte }
type plain struct { n int }

type holder struct { p *box }

func MapPointer(m map[string]*box) *box { return m["x"] }
func SlicePointer(list []*box) *box     { return list[0] }
func FieldPointer(h holder) *box        { return h.p }
func PlainPointer(list []*plain) *plain { return list[0] }

func RangeInterface(list []any) any {
	for _, v := range list { return v }
	return nil
}
`)
	views, sig := functionViews(t, analysis, pkg, "MapPointer")
	assertResultView(t, views, sig, 0, "m")
	views, sig = functionViews(t, analysis, pkg, "SlicePointer")
	assertResultView(t, views, sig, 0, "list")
	views, sig = functionViews(t, analysis, pkg, "FieldPointer")
	assertResultView(t, views, sig, 0, "h")
	views, sig = functionViews(t, analysis, pkg, "PlainPointer")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "RangeInterface")
	assertResultView(t, views, sig, 0, "list")
}

// Nothing of a bodyless callee was seen, so nothing about it reads as fresh.
func TestViewsMarkBodylessCalleesOpaque(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func external(b []byte) []byte
func externalFill(rows [][]byte)

func Wrapped(b []byte) []byte { return external(b) }

func HandedLocal() [][]byte {
	out := make([][]byte, 1)
	externalFill(out)
	return out
}

func HandedElement(s []byte) [][]byte {
	out := [][]byte{s}
	external(out[0])
	return out
}

func BodylessValue() func([]byte) []byte { return external }
`)
	views, _ := functionViews(t, analysis, pkg, "Wrapped")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "HandedLocal")
	assertOpaque(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "HandedElement")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "BodylessValue")
	assertOpaque(t, views, 0, true)
}

// Unseen writes go opaque, discoverable aliasing stays precise, proven-fresh
// forms stay fresh.
func TestViewsFailClosedCategories(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func fill(dst [][]byte, v []byte) { dst[0] = v }
func handler() {}

func DeferWritten(s []byte) [][]byte {
	out := make([][]byte, 1)
	defer fill(out, s)
	return out
}

func BoxedEscape(s []byte) [][]byte {
	out := make([][]byte, 1)
	var v any = out
	v.([][]byte)[0] = s
	return out
}

func StoredIntoParam(dst [][][]byte, s []byte) [][]byte {
	out := [][]byte{s}
	dst[0] = out
	return out
}

func ArrayPointerStore(s []byte) [][]byte {
	out := make([][]byte, 4)
	a := (*[4][]byte)(out)
	a[0] = s
	return out
}

func CapturedReloadStore(s []byte) [][]byte {
	out := make([][]byte, 1)
	defer func() { _ = out }()
	out[0] = s
	return out
}

func Handler() func()      { return handler }
func NilRows() [][]byte    { return nil }
`)
	views, _ := functionViews(t, analysis, pkg, "DeferWritten")
	assertOpaque(t, views, 0, true)
	views, sigBoxed := functionViews(t, analysis, pkg, "BoxedEscape")
	assertResultView(t, views, sigBoxed, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "StoredIntoParam")
	assertOpaque(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "ArrayPointerStore")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "CapturedReloadStore")
	assertResultView(t, views, sig, 0, "[s]")
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "Handler")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "NilRows")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
}

// A returned function value exposes what its body returns or hands out.
func TestViewsAccountFunctionValueBodies(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
var cache = [][]byte{{1}}

func yieldGlobal(yield func([]byte) bool) { yield(cache[0]) }
func emit(yield func([]byte) bool)        { yield(cache[0]) }

func GlobalIterator() func(func([]byte) bool) { return yieldGlobal }
func DirectGlobal() func() []byte             { return func() []byte { return cache[0] } }
func IndirectYield() func(func([]byte) bool) {
	return func(yield func([]byte) bool) { emit(yield) }
}
func RecvClosure(ch chan []byte) func() []byte {
	return func() []byte { return <-ch }
}
func CaptureYield(s []byte) func(func([]byte) bool) {
	return func(yield func([]byte) bool) { yield(s[1:]) }
}
`)
	views, _ := functionViews(t, analysis, pkg, "GlobalIterator")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "DirectGlobal")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "IndirectYield")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "RecvClosure")
	assertOpaque(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "CaptureYield")
	assertResultView(t, views, sig, 0, "[s]")
	assertShared(t, views, 0, false)
	assertOpaque(t, views, 0, false)
}

// A closure-written local resolves through the closure's actual stores.
func TestViewsResolveClosureStoresFromTheirSources(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
var cache = [][]byte{{1}}

func GlobalViaClosure() [][]byte {
	out := make([][]byte, 1)
	func() { out[0] = cache[0] }()
	return out
}

func RecvViaClosure(ch chan []byte) [][]byte {
	out := make([][]byte, 1)
	func() { out[0] = <-ch }()
	return out
}

func NestedViaClosure(s []byte) [][]byte {
	out := make([][]byte, 1)
	func() {
		func() { out[0] = s }()
	}()
	return out
}
`)
	views, _ := functionViews(t, analysis, pkg, "GlobalViaClosure")
	assertShared(t, views, 0, true)
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "RecvViaClosure")
	assertOpaque(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "NestedViaClosure")
	assertOpaque(t, views, 0, true)
}

// Rebinding lands at whole depth and a struct-field destination goes opaque.
func TestViewsKeepClosureStoreDestinationsApart(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
var cache = [][]byte{{1}}

func ReboundWhole(x [][]byte) [][]byte {
	var out [][]byte
	set := func(v [][]byte) { out = v }
	set(x)
	return out
}

func SiblingUntouched(s []byte) []byte {
	var p struct{ a, b []byte }
	func() { p.a = s }()
	return p.b
}

func ReboundNotElementTainted(s []byte) []byte {
	out := [][]byte{s}
	func() { out = cache }()
	return out[0]
}
`)
	views, sig := functionViews(t, analysis, pkg, "ReboundWhole")
	assertResultView(t, views, sig, 0, "x")
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "SiblingUntouched")
	assertOpaque(t, views, 0, true)
	views, sig = functionViews(t, analysis, pkg, "ReboundNotElementTainted")
	assertResultView(t, views, sig, 0, "s")
	assertShared(t, views, 0, true)
}

// A result read from a global carries the shared marker, which no override clears.
func TestViewsMarkGlobalStorageShared(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
var cache []byte
var registry = map[string]int{}
var wrapped = struct{ rows [][]byte }{}
var sentinel error

func Cached() []byte            { return cache }
func Registry() map[string]int  { return registry }
func WrappedRow() []byte        { return wrapped.rows[0] }
func Sentinel() error           { return sentinel }
func Wrapper() []byte           { return Cached() }
func CopiedGlobal() []byte      { return append([]byte(nil), cache...) }
`)
	views, _ := functionViews(t, analysis, pkg, "Cached")
	assertShared(t, views, 0, true)
	assertOpaque(t, views, 0, false)
	views, _ = functionViews(t, analysis, pkg, "Registry")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "WrappedRow")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "Sentinel")
	assertShared(t, views, 0, true)
	views, _ = functionViews(t, analysis, pkg, "Wrapper")
	assertShared(t, views, 0, true)
	views, sig := functionViews(t, analysis, pkg, "CopiedGlobal")
	assertResultView(t, views, sig, 0)
	assertShared(t, views, 0, false)
	assertOpaque(t, views, 0, false)

	derived, sig := functionViews(t, analysis, pkg, "Cached")
	resolved, invalid := ResolveViews(derived, []string{}, true, sig)
	assertNoInvalidOverrides(t, invalid)
	assertShared(t, resolved, 0, true)
	if resolved.Fresh() {
		t.Error("an override must not clear the shared marker")
	}
}

// Dynamic dispatch contributes nothing, the blind spot distinct from opaque.
func TestViewsKeepDynamicDispatchAsBlindSpot(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Provider interface{ Get() []byte }

func FromInterface(p Provider) []byte { return p.Get() }
func FromFuncValue(f func() []byte) []byte { return f() }
`)
	views, sig := functionViews(t, analysis, pkg, "FromInterface")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
	views, sig = functionViews(t, analysis, pkg, "FromFuncValue")
	assertResultView(t, views, sig, 0)
	assertOpaque(t, views, 0, false)
}

func TestViewsSettleMutualRecursion(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func viewA(s []byte, n int) []byte { if n > 0 { return viewB(s, n-1) }; return s }
func viewB(s []byte, n int) []byte { if n > 0 { return viewA(s, n-1) }; return nil }

func EntryViewA(s []byte) []byte { return viewA(s, 2) }
func EntryViewB(s []byte) []byte { return viewB(s, 2) }
`)
	views, sig := functionViews(t, analysis, pkg, "EntryViewA")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "EntryViewB")
	assertResultView(t, views, sig, 0, "s")
}

func TestViewsResolveGenericCores(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
import "iter"

func ClipShape[S ~[]E, E any](s S) S           { return s[:len(s):len(s)] }
func InsertShape[S ~[]E, E any](s S, v ...E) S { return append(s, v...) }

func AppendSeqShape[S ~[]E, E any](s S, seq iter.Seq[E]) S {
	for v := range seq { s = append(s, v) }
	return s
}
`)
	views, sig := functionViews(t, analysis, pkg, "ClipShape")
	assertResultView(t, views, sig, 0, "s")
	views, sig = functionViews(t, analysis, pkg, "InsertShape")
	assertResultView(t, views, sig, 0, "s", "[v]")
	views, sig = functionViews(t, analysis, pkg, "AppendSeqShape")
	assertResultView(t, views, sig, 0, "s")
}

func assertNoInvalidOverrides(t *testing.T, invalid []string) {
	t.Helper()
	if len(invalid) > 0 {
		t.Fatalf("unexpected invalid overrides %v", invalid)
	}
}

func TestResolveViewsAppliesWorstCaseToUnseenFunctions(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Opaque(buf []byte, n int) ([]byte, error) { return buf, nil }
`)
	_, sig := functionViews(t, analysis, pkg, "Opaque")

	resolved, invalid := ResolveViews(FunctionViews{Results: make([]ResultView, 2)}, nil, false, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0, "buf")
	assertResultView(t, resolved, sig, 1, "buf")
	assertOpaque(t, resolved, 0, true)
	assertOpaque(t, resolved, 1, true)

	resolved, invalid = ResolveViews(FunctionViews{Results: make([]ResultView, 2)}, []string{}, true, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0)
	assertResultView(t, resolved, sig, 1)
	assertOpaque(t, resolved, 0, false)
	assertOpaque(t, resolved, 1, false)
	if !resolved.Fresh() {
		t.Error("an override asserting freshness must resolve to fresh")
	}
}

func TestResolveViewsResolvesOpaqueResults(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Opaque(a, b []byte) ([]byte, []byte) { return a, nil }
`)
	derived, sig := functionViews(t, analysis, pkg, "Opaque")
	derived.Results[1].Opaque = true

	resolved, invalid := ResolveViews(derived, nil, false, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0, "a")
	assertResultView(t, resolved, sig, 1, "a", "b")
	assertOpaque(t, resolved, 1, true)

	resolved, invalid = ResolveViews(derived, []string{"1:[b]"}, true, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0, "a")
	assertResultView(t, resolved, sig, 1, "[b]")
	assertOpaque(t, resolved, 1, false)

	resolved, _ = ResolveViews(derived, []string{"1:junk"}, true, sig)
	assertResultView(t, resolved, sig, 1, "a", "b")
	assertOpaque(t, resolved, 1, true)
}

func TestResolveViewsParsesOverrideEntries(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Buffer struct { buf []byte }

func (b *Buffer) Opaque(dst []byte, n int) ([]byte, []byte) { return dst, nil }
`)
	_, sig := methodViews(t, analysis, pkg, "Buffer", "Opaque")

	unseen := FunctionViews{Results: make([]ResultView, 2)}
	resolved, invalid := ResolveViews(unseen, []string{"0:dst", "1:[recv]"}, true, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0, "dst")
	assertResultView(t, resolved, sig, 1, "[recv]")

	for _, entry := range []string{"0:n", "2:dst", "junk", "0:missing", "0:[dst"} {
		resolved, invalid = ResolveViews(unseen, []string{entry}, true, sig)
		if len(invalid) != 1 || invalid[0] != entry {
			t.Errorf("entry %q: reported invalid %v, want [%s]", entry, invalid, entry)
		}
		assertResultView(t, resolved, sig, 0, "dst", "recv")
		assertResultView(t, resolved, sig, 1, "dst", "recv")
	}
}

func TestResolveViewsUnionsOverridesOntoDerivedFacts(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Partial(a, b []byte) []byte { return a }
`)
	derived, sig := functionViews(t, analysis, pkg, "Partial")

	resolved, invalid := ResolveViews(derived, []string{"0:b"}, true, sig)
	assertNoInvalidOverrides(t, invalid)
	assertResultView(t, resolved, sig, 0, "a", "b")
	assertResultView(t, derived, sig, 0, "a")
}

func TestResolveViewsReportsInvalidOverridesOnAnalyzedFunctions(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
func Partial(a, b []byte) []byte { return a }
`)
	derived, sig := functionViews(t, analysis, pkg, "Partial")

	resolved, invalid := ResolveViews(derived, []string{"0:missing"}, true, sig)
	if len(invalid) != 1 || invalid[0] != "0:missing" {
		t.Errorf("reported invalid %v, want [0:missing]", invalid)
	}
	assertResultView(t, resolved, sig, 0, "a")
}
