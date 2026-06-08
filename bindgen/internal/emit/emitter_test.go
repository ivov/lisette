package emit

import (
	"testing"

	"github.com/ivov/lisette/bindgen/internal/convert"
)

func TestComputePkgAliasesReservesSelfNameForSingleSegmentImport(t *testing.T) {
	aliases := computePkgAliases(convert.ExternalPkgs{"io": "io"}, "io")
	if got := aliases["io"]; got == "io" || got == "" {
		t.Fatalf("import sharing the current package name must not claim the bare alias, got %q", got)
	}
}

func TestComputePkgAliasesLeavesNonCollidingImportsBare(t *testing.T) {
	aliases := computePkgAliases(convert.ExternalPkgs{"io": "io", "fmt": "fmt"}, "huh")
	if aliases["io"] != "io" || aliases["fmt"] != "fmt" {
		t.Fatalf("non-colliding imports should keep bare names, got %v", aliases)
	}
}

func TestComputePkgAliasesAlwaysAvoidsSelfAndDuplicates(t *testing.T) {
	cases := []struct {
		self string
		pkgs convert.ExternalPkgs
	}{
		{"io", convert.ExternalPkgs{"io": "io"}},
		{"widgets", convert.ExternalPkgs{"example.com/a/widgets": "widgets"}},
		{"fmt", convert.ExternalPkgs{"fmt": "fmt", "text/fmt": "fmt"}},
	}
	for _, c := range cases {
		aliases := computePkgAliases(c.pkgs, c.self)
		seen := map[string]bool{c.self: true}
		for path, alias := range aliases {
			if alias == "" {
				t.Fatalf("self=%q path=%q produced an empty alias", c.self, path)
			}
			if seen[alias] {
				t.Fatalf("self=%q produced colliding alias %q for path %q", c.self, alias, path)
			}
			seen[alias] = true
		}
	}
}
