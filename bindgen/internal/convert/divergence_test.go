package convert

import (
	"os"
	"path/filepath"

	"go/types"
	"testing"

	"github.com/ivov/lisette/bindgen/internal/config"
	"golang.org/x/tools/go/packages"
)

func analyzeDivergence(t *testing.T, source string) (*DivergenceAnalysis, *packages.Package) {
	t.Helper()
	return analyzeDivergenceWithConfig(t, nil, source)
}

func analyzeDivergenceWithConfig(t *testing.T, cfg *config.Config, source string) (*DivergenceAnalysis, *packages.Package) {
	t.Helper()
	return analyzeDivergenceFiles(t, cfg, map[string]string{"probe.go": "package probe\n\n" + source})
}

func analyzeDivergenceFiles(t *testing.T, cfg *config.Config, files map[string]string) (*DivergenceAnalysis, *packages.Package) {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte("module probe\n\ngo 1.25\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	for name, content := range files {
		full := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	pkgCfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(pkgCfg, "./...")
	if err != nil {
		t.Fatal(err)
	}
	var root *packages.Package
	for _, pkg := range pkgs {
		if len(pkg.Errors) > 0 {
			t.Fatalf("load failed for %s: %v", pkg.PkgPath, pkg.Errors)
		}
		if pkg.PkgPath == "probe" {
			root = pkg
		}
	}
	if root == nil {
		t.Fatal("root package \"probe\" not found")
	}

	nilness := NewNilnessAnalysis(pkgs, cfg)
	if nilness == nil {
		t.Fatal("SSA build failed")
	}
	analysis := NewDivergenceAnalysis(nilness, pkgs, cfg)
	if analysis == nil {
		t.Fatal("divergence analysis unavailable")
	}
	return analysis, root
}

func assertDiverges(t *testing.T, analysis *DivergenceAnalysis, pkg *packages.Package, name string, want bool) {
	t.Helper()
	obj, _ := pkg.Types.Scope().Lookup(name).(*types.Func)
	if obj == nil {
		t.Fatalf("no function %q", name)
	}
	got, ok := analysis.Function(obj)
	if !ok {
		t.Fatalf("no verdict for %q", name)
	}
	if got != want {
		t.Errorf("%s: diverges = %v, want %v", name, got, want)
	}
}

func TestDivergenceDetectsUnconditionalPanic(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
func AlwaysPanics()          { panic("boom") }
func ReturnsNormally()       {}
func PanicsOneBranch(b bool) { if b { panic("boom") } }
`)
	assertDiverges(t, analysis, pkg, "AlwaysPanics", true)
	assertDiverges(t, analysis, pkg, "ReturnsNormally", false)
	assertDiverges(t, analysis, pkg, "PanicsOneBranch", false)
}

func TestDivergenceDetectsInfiniteLoop(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
func Spins()                            { for {} }
func SpinsWithBreak()                   { for { break } }
func SpinsConditional(done func() bool) { for { if done() { break } } }
`)
	assertDiverges(t, analysis, pkg, "Spins", true)
	assertDiverges(t, analysis, pkg, "SpinsWithBreak", false)
	assertDiverges(t, analysis, pkg, "SpinsConditional", false)
}

func TestDivergencePropagatesThroughDirectCalls(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
func bottom()             { panic("boom") }
func doStuff()             {}
func Direct()               { bottom() }
func Indirect()             { Direct() }
func Conditional(b bool)    { if b { bottom() } }
func AfterCall()            { bottom(); doStuff() }
`)
	assertDiverges(t, analysis, pkg, "Direct", true)
	assertDiverges(t, analysis, pkg, "Indirect", true)
	assertDiverges(t, analysis, pkg, "Conditional", false)
	assertDiverges(t, analysis, pkg, "AfterCall", true)
}

func TestDivergenceDoesNotPropagateThroughInterfaceDispatch(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
type diverger interface{ Run() }
type concrete struct{}
func (concrete) Run() { panic("boom") }

func ThroughInterface(d diverger) { d.Run() }
func ThroughConcrete()            { concrete{}.Run() }
`)
	assertDiverges(t, analysis, pkg, "ThroughInterface", false)
	assertDiverges(t, analysis, pkg, "ThroughConcrete", true)
}

func TestDivergenceDoesNotPropagateThroughInternalPackages(t *testing.T) {
	analysis, pkg := analyzeDivergenceFiles(t, nil, map[string]string{
		"internal/intrinsic/intrinsic.go": `package intrinsic

func AlwaysPanics() { panic("intrinsic stand-in") }
`,
		"probe.go": `package probe

import "probe/internal/intrinsic"

func doStuff() {}
func ThroughInternal() { intrinsic.AlwaysPanics(); doStuff() }
`,
	})
	assertDiverges(t, analysis, pkg, "ThroughInternal", false)
}

func TestDivergenceAxiomOverridesBody(t *testing.T) {
	cfg := &config.Config{Overrides: config.Overrides{Types: config.TypeOverrides{
		NeverReturn: map[string][]string{"probe": {"CustomExit"}},
	}}}
	analysis, pkg := analyzeDivergenceWithConfig(t, cfg, `
func CustomExit()  { println("pretend this never returns") }
func AfterCall()    {}
func Caller()       { CustomExit(); AfterCall() }
`)
	assertDiverges(t, analysis, pkg, "CustomExit", true)
	assertDiverges(t, analysis, pkg, "Caller", true)
}

func TestDivergenceSettlesMutualRecursion(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
func bottom()    { panic("boom") }
func cycA(n int) { if n > 0 { cycB(n - 1) }; bottom() }
func cycB(n int) { if n > 0 { cycA(n - 1) } else { cycA(0) } }

func EntryA() { cycA(1) }
func EntryB() { cycB(1) }
`)
	assertDiverges(t, analysis, pkg, "EntryA", true)
	assertDiverges(t, analysis, pkg, "EntryB", true)
}

func TestDivergenceAnyRecoverBlockSuppressesDivergence(t *testing.T) {
	analysis, pkg := analyzeDivergence(t, `
var handler = func() { recover() }

func cleanup()          {}
func StaticRecover()    { defer func() { recover() }(); panic("boom") }
func StaticNonRecover() { defer cleanup(); panic("boom") }
func DynamicRecover()   { defer handler(); panic("boom") }
`)
	assertDiverges(t, analysis, pkg, "StaticRecover", false)
	assertDiverges(t, analysis, pkg, "StaticNonRecover", false)
	assertDiverges(t, analysis, pkg, "DynamicRecover", false)
}

func TestDivergenceAxiomsApplyWithoutConfig(t *testing.T) {
	analysis, pkg := analyzeDivergenceFiles(t, nil, map[string]string{
		"probe.go": `package probe

import (
	"os"
	"runtime"
)

func doStuff() {}

func DelegatesToExit()   { os.Exit(1); doStuff() }
func DelegatesToGoexit() { runtime.Goexit(); doStuff() }
`,
	})
	assertDiverges(t, analysis, pkg, "DelegatesToExit", true)
	assertDiverges(t, analysis, pkg, "DelegatesToGoexit", true)
}

func TestIsBuiltinDivergenceAxiomIndependentOfAnalysis(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte("module probe\n\ngo 1.25\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	source := "package probe\n\nimport (\n\t_ \"runtime\"\n\t_ \"syscall\"\n)\n"
	if err := os.WriteFile(filepath.Join(dir, "probe.go"), []byte(source), 0o644); err != nil {
		t.Fatal(err)
	}

	pkgCfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(pkgCfg, "./...")
	if err != nil {
		t.Fatal(err)
	}

	var syscallExit, runtimeGoexit *types.Func
	seen := make(map[*packages.Package]bool)
	var walk func(*packages.Package)
	walk = func(pkg *packages.Package) {
		if pkg == nil || pkg.Types == nil || seen[pkg] {
			return
		}
		seen[pkg] = true
		switch pkg.PkgPath {
		case "syscall":
			syscallExit, _ = pkg.Types.Scope().Lookup("Exit").(*types.Func)
		case "runtime":
			runtimeGoexit, _ = pkg.Types.Scope().Lookup("Goexit").(*types.Func)
		}
		for _, imp := range pkg.Imports {
			walk(imp)
		}
	}
	for _, pkg := range pkgs {
		walk(pkg)
	}
	if syscallExit == nil || runtimeGoexit == nil {
		t.Fatal("could not resolve syscall.Exit / runtime.Goexit")
	}

	if !isBuiltinDivergenceAxiom(syscallExit) {
		t.Error("syscall.Exit: isBuiltinDivergenceAxiom = false, want true")
	}
	if !isBuiltinDivergenceAxiom(runtimeGoexit) {
		t.Error("runtime.Goexit: isBuiltinDivergenceAxiom = false, want true")
	}
}
