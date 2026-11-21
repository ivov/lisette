package cli

import (
	"encoding/json"
	"fmt"
	"maps"
	"os"
	"slices"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/importscan"
)

type ImportsLocation struct {
	Path string `json:"path"`
	Dir  string `json:"dir"`
}

type ImportsRequest struct {
	Modules  []ImportsLocation `json:"modules"`
	Packages []ImportsLocation `json:"packages"`
}

type ImportsResponse struct {
	Packages []importscan.Package `json:"packages"`
}

// RunImports reads an ImportsRequest on stdin and prints each package's imports for every target.
func RunImports(args []string) {
	if len(args) > 0 {
		fmt.Fprintf(os.Stderr, "Usage: bindgen imports < request.json\n")
		os.Exit(2)
	}

	var request ImportsRequest
	if err := json.NewDecoder(os.Stdin).Decode(&request); err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to read imports request: %v\n", err)
		os.Exit(1)
	}

	scanned := make(map[string]importscan.Package)
	for _, module := range request.Modules {
		packages, err := importscan.ModulePackages(module.Path, module.Dir)
		if err != nil {
			fmt.Fprintf(os.Stderr, "bindgen: failed to list packages of %s: %v\n", module.Path, err)
			os.Exit(1)
		}
		for _, pkg := range packages {
			scanned[pkg.Path] = pkg
		}
	}
	for _, location := range request.Packages {
		scanned[location.Path], _ = importscan.Scan(location.Path, location.Dir)
	}

	response := ImportsResponse{
		Packages: slices.SortedFunc(maps.Values(scanned), func(a, b importscan.Package) int {
			return strings.Compare(a.Path, b.Path)
		}),
	}
	if err := json.NewEncoder(os.Stdout).Encode(response); err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to write imports: %v\n", err)
		os.Exit(1)
	}
}
