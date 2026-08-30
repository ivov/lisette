package convert

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/tools/go/packages"
)

func TestNilnessAnalysisPromotedFieldLiteral(t *testing.T) {
	dir := t.TempDir()
	write := func(name, content string) {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("go.mod", "module probe\n\ngo 1.27\n")
	write("probe.go", `package probe

type header struct {
	Name string
}

type Record struct {
	header
	Count int
}

func NewRecord(name string) *Record {
	return &Record{Name: name, Count: 1}
}
`)

	cfg := &packages.Config{Mode: packages.LoadAllSyntax, Dir: dir}
	pkgs, err := packages.Load(cfg, "probe")
	if err != nil {
		if strings.Contains(err.Error(), "requires go >= 1.27") {
			t.Skipf("toolchain predates promoted fields in struct literals: %v", err)
		}
		t.Fatal(err)
	}
	if len(pkgs) != 1 {
		t.Fatalf("load failed: %v", pkgs)
	}
	if len(pkgs[0].Errors) > 0 {
		t.Skipf("toolchain predates promoted fields in struct literals: %v", pkgs[0].Errors[0])
	}

	nilness, err := NewNilnessAnalysis(pkgs, nil)
	if err != nil || nilness == nil {
		t.Fatalf("SSA build failed on a promoted-field struct literal: %v", err)
	}
	facts, ok := nilness.Function(lookupFunc(t, pkgs[0], "NewRecord"))
	if !ok {
		t.Fatal("no verdict for NewRecord")
	}
	if facts.Single != ReturnProvenNonNil {
		t.Fatalf("NewRecord: got %v, want ReturnProvenNonNil", facts.Single)
	}
}
