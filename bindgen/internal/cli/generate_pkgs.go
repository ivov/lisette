package cli

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"
	"sort"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/sync/errgroup"
	"golang.org/x/tools/go/packages"
)

type ManifestErrorKind string

const (
	KindListError    ManifestErrorKind = "list_error"
	KindUnknownError ManifestErrorKind = "unknown_error"
	KindLoadFailed   ManifestErrorKind = "load_failed"
)

type ManifestOk struct {
	Package string `json:"package"`
	Content string `json:"content"`
	Stubbed bool   `json:"stubbed"`
}

// Hard-fails only — soft-fail type-check errors route through
// generateUnloadableStub and end up in Ok with Stubbed=true.
type ManifestError struct {
	Package string            `json:"package"`
	Kind    ManifestErrorKind `json:"kind"`
	Message string            `json:"message"`
}

type Manifest struct {
	Ok     []ManifestOk    `json:"ok"`
	Errors []ManifestError `json:"errors"`
}

func RunPkgs(args []string, defaultCfgJSON []byte) {
	fs := flag.NewFlagSet("pkgs", flag.ExitOnError)
	configPath := fs.String("config", "", "path to bindgen config file")
	versionOverride := fs.String("version", "", "override Lisette version in generated headers")
	transitive := fs.Bool("transitive", false, "also emit the requested packages' transitive re-exports")
	fs.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: bindgen pkgs [-config <path>] [-version <ver>] [-transitive]\n\n")
		fmt.Fprintf(os.Stderr, "Generates .d.lis type definitions for many Go packages in one shared\n")
		fmt.Fprintf(os.Stderr, "type-check pass. Reads package paths from stdin, one per line. Emits a\n")
		fmt.Fprintf(os.Stderr, "JSON manifest on stdout with embedded content.\n")
	}

	_ = fs.Parse(args)

	pkgPaths, err := readPackageList(os.Stdin)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to read package list: %v\n", err)
		os.Exit(1)
	}

	cfg, err := config.LoadConfig(*configPath, defaultCfgJSON)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to load config: %v\n", err)
		os.Exit(1)
	}

	effectiveVersion := lisVersion
	if *versionOverride != "" {
		effectiveVersion = *versionOverride
	}

	manifest := GeneratePkgs(pkgPaths, effectiveVersion, goVersion, &cfg, *transitive)

	if err := json.NewEncoder(os.Stdout).Encode(manifest); err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to encode manifest: %v\n", err)
		os.Exit(1)
	}
}

func GeneratePkgs(pkgPaths []string, lisetteVersion, goVersion string, cfg *config.Config, transitive bool) Manifest {
	manifest := Manifest{
		Ok:     make([]ManifestOk, 0, len(pkgPaths)),
		Errors: make([]ManifestError, 0),
	}

	if len(pkgPaths) == 0 {
		return manifest
	}

	pkgs, err := extract.LoadPackagesAll(pkgPaths)
	if err != nil {
		for _, p := range pkgPaths {
			manifest.Errors = append(manifest.Errors, ManifestError{
				Package: p,
				Kind:    KindLoadFailed,
				Message: err.Error(),
			})
		}
		return manifest
	}

	// In transitive mode, index the full graph so re-exports generate from this Load.
	byPath := make(map[string]*packages.Package)
	var indexGraph func(pkg *packages.Package)
	indexGraph = func(pkg *packages.Package) {
		if pkg == nil || byPath[pkg.PkgPath] != nil {
			return
		}
		byPath[pkg.PkgPath] = pkg
		if transitive {
			for _, imp := range pkg.Imports {
				indexGraph(imp)
			}
		}
	}
	for _, pkg := range pkgs {
		indexGraph(pkg)
	}

	visited := make(map[string]bool)
	frontier := make([]string, 0, len(pkgPaths))
	for _, input := range pkgPaths {
		pkg := byPath[input]
		if pkg == nil {
			manifest.Errors = append(manifest.Errors, ManifestError{
				Package: input,
				Kind:    KindLoadFailed,
				Message: "no package found",
			})
			continue
		}
		if hardErr := firstHardError(pkg); hardErr != nil {
			manifest.Errors = append(manifest.Errors, *hardErr)
			continue
		}
		if !visited[input] {
			visited[input] = true
			frontier = append(frontier, input)
		}
	}

	for len(frontier) > 0 {
		type waveResult struct {
			ok      ManifestOk
			imports []string
		}
		wave := make([]waveResult, len(frontier))

		g := new(errgroup.Group)
		g.SetLimit(runtime.NumCPU())
		for i, pkgPath := range frontier {
			i, pkgPath := i, pkgPath
			pkg := byPath[pkgPath]
			g.Go(func() error {
				if len(pkg.Errors) > 0 {
					stub := generateUnloadableStub(pkgPath, pkg, lisetteVersion, goVersion)
					wave[i] = waveResult{ok: ManifestOk{Package: pkgPath, Content: stub.Content, Stubbed: true}}
				} else {
					result := generateFromPackage(pkg, pkgPath, lisetteVersion, goVersion, cfg)
					wave[i] = waveResult{
						ok:      ManifestOk{Package: pkgPath, Content: result.Content, Stubbed: false},
						imports: result.ExternalImports,
					}
				}
				return nil
			})
		}
		_ = g.Wait()

		var next []string
		for _, w := range wave {
			manifest.Ok = append(manifest.Ok, w.ok)
			if !transitive {
				continue
			}
			for _, imp := range w.imports {
				if visited[imp] || !isThirdPartyPackage(imp) || extract.IsInternalPackagePath(imp) {
					continue
				}
				pkg := byPath[imp]
				if pkg == nil || firstHardError(pkg) != nil {
					continue
				}
				visited[imp] = true
				next = append(next, imp)
			}
		}
		frontier = next
	}

	sort.Slice(manifest.Ok, func(i, j int) bool {
		return manifest.Ok[i].Package < manifest.Ok[j].Package
	})

	return manifest
}

func isThirdPartyPackage(path string) bool {
	first := path
	if before, _, ok := strings.Cut(path, "/"); ok {
		first = before
	}
	return strings.Contains(first, ".")
}

func firstHardError(pkg *packages.Package) *ManifestError {
	for _, e := range pkg.Errors {
		switch e.Kind {
		case packages.ListError:
			return &ManifestError{Package: pkg.PkgPath, Kind: KindListError, Message: e.Msg}
		case packages.UnknownError:
			return &ManifestError{Package: pkg.PkgPath, Kind: KindUnknownError, Message: e.Msg}
		}
	}
	return nil
}

func readPackageList(r io.Reader) ([]string, error) {
	var paths []string
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		paths = append(paths, line)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	return paths, nil
}
