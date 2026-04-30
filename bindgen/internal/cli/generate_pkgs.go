package cli

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"
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
	fs.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: bindgen pkgs [-config <path>] [-version <ver>]\n\n")
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

	manifest := GeneratePkgs(pkgPaths, effectiveVersion, goVersion, &cfg)

	if err := json.NewEncoder(os.Stdout).Encode(manifest); err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to encode manifest: %v\n", err)
		os.Exit(1)
	}
}

func GeneratePkgs(pkgPaths []string, lisetteVersion, goVersion string, cfg *config.Config) Manifest {
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

	byPath := make(map[string]*packages.Package, len(pkgs))
	for _, pkg := range pkgs {
		if pkg == nil {
			continue
		}
		byPath[pkg.PkgPath] = pkg
	}

	// Pre-allocated per-input slot avoids a mutex around append. Indices with
	// no enqueued goroutine stay at the zero value and are filtered out below.
	results := make([]ManifestOk, len(pkgPaths))
	enqueued := make([]bool, len(pkgPaths))

	g := new(errgroup.Group)
	g.SetLimit(runtime.NumCPU())

	for i, input := range pkgPaths {
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

		i, input, pkg := i, input, pkg
		enqueued[i] = true
		g.Go(func() error {
			if len(pkg.Errors) > 0 {
				stub := generateUnloadableStub(input, pkg, lisetteVersion, goVersion)
				results[i] = ManifestOk{Package: input, Content: stub.Content, Stubbed: true}
			} else {
				result := generateFromPackage(pkg, input, lisetteVersion, goVersion, cfg)
				results[i] = ManifestOk{Package: input, Content: result.Content, Stubbed: false}
			}
			return nil
		})
	}

	_ = g.Wait()

	for i, ok := range enqueued {
		if ok {
			manifest.Ok = append(manifest.Ok, results[i])
		}
	}

	return manifest
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
