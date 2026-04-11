package cli

import (
	"fmt"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/extract"
)

// GeneratePkg generates a `.d.lis` file for a Go package path.
func GeneratePkg(pkgPath, lisetteVersion, goVersion string, cfg *config.Config) (GeneratePkgResult, error) {
	pkg, err := extract.LoadPackage(pkgPath)
	if err != nil {
		return GeneratePkgResult{}, fmt.Errorf("failed to load package %s: %w", pkgPath, err)
	}
	if pkg == nil {
		return GeneratePkgResult{}, fmt.Errorf("no package found at %s", pkgPath)
	}

	return generateFromPackage(pkg, pkgPath, lisetteVersion, goVersion, cfg), nil
}
