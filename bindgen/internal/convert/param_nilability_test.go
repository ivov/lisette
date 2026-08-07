package convert

import (
	"os"
	"path/filepath"
	"strings"

	"go/types"
	"testing"

	"github.com/ivov/lisette/bindgen/internal/config"
	"golang.org/x/tools/go/packages"
)

func nilabilitySource(t *testing.T, source string, bindgenConfig *config.Config) (*NilnessAnalysis, *packages.Package) {
	t.Helper()
	dir := t.TempDir()
	write := func(name, content string) {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("go.mod", "module probe\n\ngo 1.25\n")
	write("probe.go", "package probe\n\n"+source+shared)

	cfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(cfg, "probe")
	if err != nil {
		t.Fatal(err)
	}
	if len(pkgs) != 1 || len(pkgs[0].Errors) > 0 {
		t.Fatalf("load failed: %v", pkgs)
	}
	nilness := NewNilnessAnalysis(pkgs, bindgenConfig)
	if nilness == nil {
		t.Fatal("SSA build failed")
	}
	return nilness, pkgs[0]
}

func tolerantParams(t *testing.T, nilness *NilnessAnalysis, pkg *packages.Package, name string) []string {
	t.Helper()
	obj := lookupFunc(t, pkg, name)
	sig, ok := obj.Type().(*types.Signature)
	if !ok {
		t.Fatalf("%q is not a function", name)
	}
	var out []string
	for i := 0; i < sig.Params().Len(); i++ {
		if nilness.Params().Optional(obj, i) {
			out = append(out, sig.Params().At(i).Name())
		}
	}
	return out
}

func lookupFunc(t *testing.T, pkg *packages.Package, name string) *types.Func {
	t.Helper()
	typeName, methodName, isMethod := strings.Cut(name, ".")
	if !isMethod {
		obj, _ := pkg.Types.Scope().Lookup(name).(*types.Func)
		if obj == nil {
			t.Fatalf("no function %q", name)
		}
		return obj
	}
	named, _ := pkg.Types.Scope().Lookup(typeName).(*types.TypeName)
	if named == nil {
		t.Fatalf("no type %q", typeName)
	}
	for method := range named.Type().(*types.Named).Methods() {
		if method.Name() == methodName {
			return method
		}
	}
	t.Fatalf("no method %q", name)
	return nil
}

func assertTolerant(t *testing.T, source, fn string, want ...string) {
	t.Helper()
	nilness, pkg := nilabilitySource(t, source, nil)
	got := tolerantParams(t, nilness, pkg, fn)
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("%s: tolerant params = %v, want %v", fn, got, want)
	}
}

const shared = `
type T struct{ x int }
type Node struct{ left, right *Node }
`

func TestToleratesDefaultOnNil(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) int {
	if p == nil {
		p = &T{}
	}
	return p.x
}
`, "Use", "p")
}

func TestRejectsDereference(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) int { return p.x }
`, "Use")
}

func TestRejectsConstructedErrorReturn(t *testing.T) {
	assertTolerant(t, `
import "errors"

func Use(p *T) error {
	if p == nil {
		return errors.New("p required")
	}
	return nil
}
`, "Use")
}

func TestRejectsPanic(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) {
	if p == nil {
		panic("p required")
	}
}
`, "Use")
}

func TestRejectsMixedOutcome(t *testing.T) {
	assertTolerant(t, `
import "errors"

func Use(p *T, flag bool) error {
	if p == nil {
		if flag {
			return nil
		}
		return errors.New("p required")
	}
	return nil
}
`, "Use")
}

// (None, None) would dereference nil.
func TestJointDereference(t *testing.T) {
	assertTolerant(t, `
func Use(p, q *T) int {
	if p == nil {
		return q.x
	}
	return p.x
}
`, "Use", "p")
}

func TestJointErrorGuard(t *testing.T) {
	assertTolerant(t, `
import "errors"

func Use(p, q *T) error {
	if p == nil && q == nil {
		return errors.New("invalid")
	}
	return nil
}
`, "Use")
}

func TestRejectsEscapeIntoField(t *testing.T) {
	assertTolerant(t, `
type Holder struct{ p *T }

func Use(h *Holder, p *T) { h.p = p }
`, "Use")
}

func TestRejectsEscapeIntoGlobal(t *testing.T) {
	assertTolerant(t, `
var sink *T

func Use(p *T) { sink = p }
`, "Use")
}

func TestRejectsEscapeIntoChannel(t *testing.T) {
	assertTolerant(t, `
func Use(p *T, ch chan *T) { ch <- p }
`, "Use")
}

func TestRejectsEscapeIntoClosure(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) func() int {
	return func() int { return p.x }
}
`, "Use")
}

func TestRejectsEscapeIntoMap(t *testing.T) {
	assertTolerant(t, `
func Use(m map[string]*T, p *T) { m["k"] = p }
`, "Use")
}

// A returned nil is the return side's business.
func TestReturningParameterIsNotEscape(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) *T { return p }
`, "Use", "p")
}

func TestRejectsCalleeDereference(t *testing.T) {
	assertTolerant(t, `
func helper(p *T) int { return p.x }

func Use(p *T) int { return helper(p) }
`, "Use")
}

func TestRejectsCalleePanicWithoutTouching(t *testing.T) {
	assertTolerant(t, `
func check(p *T) {
	if p == nil {
		panic("p required")
	}
}

func Use(p *T) int {
	check(p)
	return 0
}
`, "Use")
}

func TestRejectsCalleeJointGuard(t *testing.T) {
	assertTolerant(t, `
func h(p, q *T) {
	if p == nil && q == nil {
		panic("invalid")
	}
}

func Use(p, q *T) { h(p, q) }
`, "Use")
}

func TestRejectsDereferenceOfDelegatedResult(t *testing.T) {
	assertTolerant(t, `
func identity(p *T) *T { return p }

func Use(p *T) int {
	q := identity(p)
	return q.x
}
`, "Use")
}

func TestRejectsThroughDefer(t *testing.T) {
	assertTolerant(t, `
func helper(p *T) int { return p.x }

func Use(p *T) { defer helper(p) }
`, "Use")
}

func TestRejectsThroughGo(t *testing.T) {
	assertTolerant(t, `
func helper(p *T) int { return p.x }

func Use(p *T) { go helper(p) }
`, "Use")
}

func TestHelperMediatedToleranceIsNotProven(t *testing.T) {
	assertTolerant(t, `
func present(p *T) bool { return p != nil }

func Use(p *T) bool { return present(p) }
`, "Use")
}

func TestAcceptsRecursiveWalk(t *testing.T) {
	assertTolerant(t, `
func Walk(n *Node) {
	if n == nil {
		return
	}
	Walk(n.left)
	Walk(n.right)
}
`, "Walk", "n")
}

func TestMutualRecursionSettles(t *testing.T) {
	assertTolerant(t, `
func a(p *T) { b(p) }

func b(p *T) {
	if p == nil {
		panic("p required")
	}
	a(p)
}

func Use(p *T) { a(p) }
`, "Use")
}

func TestRejectsNilReceiverCallee(t *testing.T) {
	assertTolerant(t, `
func (t *T) Check() {
	if t == nil {
		panic("nil")
	}
}

func Use(t *T) { t.Check() }
`, "Use")
}

func TestReceiverNeverFlips(t *testing.T) {
	assertTolerant(t, `
func (t *T) Describe() string {
	if t == nil {
		return "none"
	}
	return "some"
}
`, "T.Describe")
}

func TestVariadicNeverFlips(t *testing.T) {
	assertTolerant(t, `
func Use(ps ...*T) int { return len(ps) }
`, "Use")
}

func TestRejectsBodylessCallee(t *testing.T) {
	assertTolerant(t, `
import "unsafe"

func opaque(p unsafe.Pointer)

func Use(p *T) { opaque(unsafe.Pointer(p)) }
`, "Use")
}

func TestRejectsCommaOkAssertThenDereference(t *testing.T) {
	assertTolerant(t, `
type Iface interface{ M() }

func (t *T) M() {}

func Use(v Iface) int {
	p, _ := v.(*T)
	return p.x
}
`, "Use")
}

func TestRejectsSliceOfNilPointerToArray(t *testing.T) {
	assertTolerant(t, `
func Use(p *[1]int) []int { return p[:] }
`, "Use")
}

func TestRejectsStoreIntoHeapLocal(t *testing.T) {
	assertTolerant(t, `
func Use(p *T) **T {
	q := p
	return &q
}
`, "Use")
}

func TestReturnPinBeatsNilableParamOverride(t *testing.T) {
	source := `
func Use(p *T) *T {
	if p == nil {
		return nil
	}
	return p
}
`
	optionalUnder := func(cfg *config.Config) bool {
		t.Helper()
		nilness, pkg := nilabilitySource(t, source, cfg)
		return nilness.Params().Optional(lookupFunc(t, pkg, "Use"), 0)
	}

	forced := &config.Config{}
	forced.Overrides.Types.NilableParam = map[string]map[string][]string{
		"probe": {"Use": {"p"}},
	}
	if !optionalUnder(forced) {
		t.Fatal("nilable_param alone should force the flip")
	}

	pinned := &config.Config{}
	pinned.Overrides.Types.NilableParam = forced.Overrides.Types.NilableParam
	pinned.Overrides.Types.NonNilableReturn = map[string][]string{"probe": {"Use"}}
	if optionalUnder(pinned) {
		t.Fatal("a non_nilable_return pin must block the flip even when nilable_param forces it")
	}

	cancelled := &config.Config{}
	cancelled.Overrides.Types.NonNilableParam = map[string]map[string][]string{
		"probe": {"Use": {"p"}},
	}
	if optionalUnder(cancelled) {
		t.Fatal("non_nilable_param must cancel an inference")
	}
}

func TestForcedOptionalNeighbourBlocksInference(t *testing.T) {
	source := `
func F(p, q *T) int {
	if q == nil {
		return p.x
	}
	return 0
}
`
	plain, pkg := nilabilitySource(t, source, nil)
	fn := lookupFunc(t, pkg, "F")
	if plain.Params().Optional(fn, 0) {
		t.Fatal("p dereferences on a live path, so it must not be proven")
	}
	if !plain.Params().Optional(fn, 1) {
		t.Fatal("q is tolerated when p is a real pointer")
	}

	forced := &config.Config{}
	forced.Overrides.Types.NilableParam = map[string]map[string][]string{
		fn.Pkg().Path(): {"F": {"p"}},
	}
	withOverride, pkg := nilabilitySource(t, source, forced)
	fn = lookupFunc(t, pkg, "F")
	if !withOverride.Params().Optional(fn, 0) {
		t.Fatal("nilable_param must force p optional")
	}
	if withOverride.Params().Optional(fn, 1) {
		t.Fatal("q must not stay proven once p can be None: F(None, None) panics")
	}
}

func TestInterfaceMethodDoesNotFlip(t *testing.T) {
	assertTolerant(t, `
type Sink interface{ Accept(p *T) bool }

type Impl struct{}

func (i *Impl) Accept(p *T) bool { return p != nil }

var _ Sink = (*Impl)(nil)
`, "Impl.Accept")
}

func TestNonInterfaceMethodStillFlips(t *testing.T) {
	assertTolerant(t, `
type Lone struct{}

func (l *Lone) Accept(p *T) bool { return p != nil }
`, "Lone.Accept", "p")
}

func TestNamedFuncTypeShapeDoesNotFlip(t *testing.T) {
	assertTolerant(t, `
type Handler func(p *T) bool

func Accept(p *T) bool { return p != nil }

var _ Handler = Accept
`, "Accept")
}

func TestPromotedInterfaceMethodDoesNotFlip(t *testing.T) {
	assertTolerant(t, `
type Sink interface {
	Accept(p *T) bool
	Extra()
}

type inner struct{}

func (i inner) Accept(p *T) bool { return p != nil }

type Outer struct {
	inner
}

func (o Outer) Extra() {}

var _ Sink = Outer{}
`, "inner.Accept")
}

func TestOptionalWithoutAnalysis(t *testing.T) {
	_, pkg := nilabilitySource(t, `
func Use(p *T) int {
	if p == nil {
		return 0
	}
	return p.x
}
`, nil)
	var absent *ParameterNilability
	if absent.Optional(lookupFunc(t, pkg, "Use"), 0) {
		t.Fatal("no analysis means nothing is proven")
	}
}
