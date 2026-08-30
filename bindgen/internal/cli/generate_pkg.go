package cli

import (
	"fmt"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/convert"
	"github.com/ivov/lisette/bindgen/internal/emit"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/tools/go/packages"
)

// GeneratePkg generates a `.d.lis` file for a Go package path.
func GeneratePkg(pkgPath, lisetteVersion, goToolchainVersion string, cfg *config.Config, target Target) (GeneratePkgResult, error) {
	pkg, err := extract.LoadPackage(pkgPath, target.goos, target.goarch)
	if err != nil {
		return GeneratePkgResult{}, fmt.Errorf("failed to load package %s: %w", pkgPath, err)
	}
	if pkg == nil {
		return GeneratePkgResult{}, fmt.Errorf("no package found at %s", pkgPath)
	}

	if len(pkg.Errors) > 0 {
		return generateUnloadableStub(pkgPath, pkg, lisetteVersion, goToolchainVersion), nil
	}

	nilness, err := convert.NewNilnessAnalysis([]*packages.Package{pkg}, cfg)
	if err != nil {
		return GeneratePkgResult{}, fmt.Errorf("analysis of %s failed: %w", pkgPath, err)
	}
	mutation, err := convert.NewMutationAnalysis(nilness, []*packages.Package{pkg})
	if err != nil {
		return GeneratePkgResult{}, fmt.Errorf("analysis of %s failed: %w", pkgPath, err)
	}
	divergence, err := convert.NewDivergenceAnalysis(nilness, []*packages.Package{pkg}, cfg)
	if err != nil {
		return GeneratePkgResult{}, fmt.Errorf("analysis of %s failed: %w", pkgPath, err)
	}
	return generateFromPackage(pkg, pkgPath, lisetteVersion, goToolchainVersion, cfg, nilness, mutation, divergence), nil
}

// generateUnloadableStub returns a header-only typedef with zero exports for
// a package that failed to type-check under CGO_ENABLED=0.
func generateUnloadableStub(pkgPath string, pkg *packages.Package, lisetteVersion, goToolchainVersion string) GeneratePkgResult {
	emitter := emit.NewEmitter(nil, pkgPath, pkg.Name, nil, nil)
	emitter.EmitHeader(pkgPath, pkg.Name, lisetteVersion, goToolchainVersion)
	emitter.EmitUnloadableNote(pkg.Errors[0].Msg)
	return GeneratePkgResult{
		Content: emitter.String(),
		Summary: fmt.Sprintf("Skipped: package %s could not be type-checked (%d error(s))\n", pkgPath, len(pkg.Errors)),
	}
}
