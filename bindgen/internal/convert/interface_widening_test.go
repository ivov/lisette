package convert

import (
	"go/types"
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/tools/go/packages"
)

func analyzePackages(t *testing.T, files map[string]string, patterns ...string) (*MutationAnalysis, map[string]*packages.Package) {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte("module probe\n\ngo 1.25\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	for name, content := range files {
		path := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	cfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(cfg, patterns...)
	if err != nil {
		t.Fatal(err)
	}
	byPath := make(map[string]*packages.Package, len(pkgs))
	for _, pkg := range pkgs {
		if len(pkg.Errors) > 0 {
			t.Fatalf("load failed: %v", pkg.Errors)
		}
		byPath[pkg.PkgPath] = pkg
	}
	nilness, err := NewNilnessAnalysis(pkgs, nil)
	if err != nil || nilness == nil {
		t.Fatalf("SSA build failed: %v", err)
	}
	analysis, err := NewMutationAnalysis(nilness, pkgs)
	if err != nil || analysis == nil {
		t.Fatalf("mutation analysis unavailable: %v", err)
	}
	return analysis, byPath
}

func interfaceMethodMutations(t *testing.T, analysis *MutationAnalysis, pkg *packages.Package, typeName, methodName string) []string {
	t.Helper()
	named := pkg.Types.Scope().Lookup(typeName).(*types.TypeName).Type().(*types.Named)
	for method := range named.Underlying().(*types.Interface).Methods() {
		if method.Name() != methodName {
			continue
		}
		facts, _ := analysis.Function(method)
		sig := method.Type().(*types.Signature)
		var out []string
		for i := 0; i < sig.Params().Len(); i++ {
			if facts.Mutates(i) {
				out = append(out, sig.Params().At(i).Name())
			}
		}
		return out
	}
	t.Fatalf("no method %s.%s", typeName, methodName)
	return nil
}

func TestInterfaceWidensAcrossPackages(t *testing.T) {
	analysis, pkgs := analyzePackages(t, map[string]string{
		"iface/iface.go": `package iface

type Box[T any] struct{ Value T }

type Plain interface {
	Apply(b *Box[string])
}

type Generic[T any] interface {
	Apply(b *Box[T])
}

type Reader interface {
	Read(b *Box[string])
}
`,
		"impl/impl.go": `package impl

import "probe/iface"

type PlainImpl struct{}

func (*PlainImpl) Apply(b *iface.Box[string]) { b.Value = "x" }

type GenericImpl[T any] struct{}

func (*GenericImpl[T]) Apply(b *iface.Box[T]) {
	var zero T
	b.Value = zero
}

type ReaderImpl struct{}

func (ReaderImpl) Read(b *iface.Box[string]) { _ = b.Value }
`,
	}, "probe/iface", "probe/impl")
	iface := pkgs["probe/iface"]
	assertMutates(t, interfaceMethodMutations(t, analysis, iface, "Plain", "Apply"), "b")
	assertMutates(t, interfaceMethodMutations(t, analysis, iface, "Generic", "Apply"), "b")
	assertMutates(t, interfaceMethodMutations(t, analysis, iface, "Reader", "Read"))
}

func TestInterfaceWideningFollowsRecordedInstantiations(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Box struct{ Value int }

type Store[T any] interface {
	Put(b *Box, value T)
}

type IntStore struct{}

func (IntStore) Put(b *Box, value int) { b.Value = value }

var _ Store[int] = IntStore{}

type Silent[T any] interface {
	Keep(b *Box, value T)
}

type IntKeeper struct{}

func (IntKeeper) Keep(b *Box, value int) { b.Value = value }

type Typed[T ~int] interface {
	Visit(b *Box, value T)
}

type StringVisitor struct{}

func (StringVisitor) Visit(b *Box, value string) { b.Value = len(value) }

type IntVisitor struct{}

func (IntVisitor) Visit(b *Box, value int) { _ = b.Value }

var _ Typed[int] = IntVisitor{}
`)
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "Store", "Put"), "b")
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "Silent", "Keep"))
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "Typed", "Visit"))
}

func TestInterfaceWideningRespectsGenericImplementerConstraints(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Box struct{ Value int }

type IntOnly[T ~int] interface {
	Fill(b *Box, value T)
}

type StringFiller[U ~string] struct{}

func (StringFiller[U]) Fill(b *Box, value U) { b.Value = 1 }

type IntAlso[T ~int] interface {
	Stamp(b *Box, value T)
}

type IntStamper[U ~int] struct{}

func (IntStamper[U]) Stamp(b *Box, value U) { b.Value = 1 }
`)
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "IntOnly", "Fill"))
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "IntAlso", "Stamp"), "b")
}

func TestInterfaceWideningReachesInstantiatedCopies(t *testing.T) {
	analysis, pkg := analyzeSource(t, `
type Box[T any] struct{ Value T }

type Generic[T any] interface {
	Apply(b *Box[T])
}

type Labeled interface {
	Generic[string]
	Label() string
}

type LabeledImpl struct{}

func (LabeledImpl) Apply(b *Box[string]) { b.Value = "x" }
func (LabeledImpl) Label() string        { return "" }
`)
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "Generic", "Apply"), "b")
	assertMutates(t, interfaceMethodMutations(t, analysis, pkg, "Labeled", "Apply"), "b")
}

func TestPromotedGenericMethodKeepsItsVerdict(t *testing.T) {
	analysis, pkgs := analyzePackages(t, map[string]string{
		"base/base.go": `package base

type Base[T any] struct{ value T }

func (b *Base[T]) Get() T { return b.value }
`,
		"wrap/wrap.go": `package wrap

import "probe/base"

type Wrapper struct{ base.Base[int] }
`,
	}, "probe/wrap")
	wrapper := pkgs["probe/wrap"].Types.Scope().Lookup("Wrapper").(*types.TypeName).Type().(*types.Named)
	selection := analysis.program.MethodSets.MethodSet(types.NewPointer(wrapper)).Lookup(nil, "Get")
	if selection == nil {
		t.Fatal("Wrapper has no promoted Get")
	}
	facts, ok := analysis.Function(selection.Obj())
	if !ok {
		t.Fatal("no verdict for the promoted Get")
	}
	if facts.ReceiverMutates {
		t.Fatal("Get does not write through its receiver")
	}
}
