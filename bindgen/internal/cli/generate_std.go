package cli

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/sync/errgroup"
)

type GenerateStdResult struct {
	Generated int
	Duration  time.Duration
}

type Target struct {
	GOOS, GOARCH string
}

func (t Target) String() string {
	return t.GOOS + "/" + t.GOARCH
}

func (t Target) Suffix() string {
	return t.GOOS + "_" + t.GOARCH
}

// GenerateStd generates per-target `.d.lis` files and deduplicates them
// into a suffixless shared layer plus per-target overlays.
func GenerateStd(ctx context.Context, outDir, lisetteVersion, goVersion string, cfg *config.Config, targets []Target) (GenerateStdResult, error) {
	start := time.Now()

	if len(targets) < 2 {
		return GenerateStdResult{}, fmt.Errorf("at least two targets are required: a single-target regen cannot distinguish platform-conditional packages from common ones")
	}

	captured := make(map[Target]map[string]string)

	for _, target := range targets {
		fmt.Fprintf(os.Stderr, "\n=== Target %s ===\n", target)

		pkgPaths, err := listStdlibPackages(target)
		if err != nil {
			return GenerateStdResult{}, fmt.Errorf("target %s: failed to list stdlib packages: %w", target, err)
		}

		loadStart := time.Now()
		pkgs, err := extract.LoadPackages(pkgPaths, target.GOOS, target.GOARCH)
		if err != nil {
			return GenerateStdResult{}, fmt.Errorf("target %s: failed to load packages: %w", target, err)
		}
		fmt.Fprintf(os.Stderr, "Loaded %d packages in %.1fs\n\n", len(pkgs), time.Since(loadStart).Seconds())

		var generated atomic.Int32
		total := len(pkgs)

		results := make(map[string]string, len(pkgs))
		var resultsMu sync.Mutex

		g, gctx := errgroup.WithContext(ctx)
		g.SetLimit(runtime.NumCPU())

		for _, pkg := range pkgs {
			g.Go(func() error {
				select {
				case <-gctx.Done():
					return gctx.Err()
				default:
				}

				result := generateFromPackage(pkg, pkg.PkgPath, lisetteVersion, goVersion, cfg)

				resultsMu.Lock()
				results[pkg.PkgPath] = result.Content
				resultsMu.Unlock()

				n := generated.Add(1)
				fmt.Fprintf(os.Stderr, "[%3d/%d] %s\n", n, total, pkg.PkgPath)

				return nil
			})
		}

		if err := g.Wait(); err != nil {
			return GenerateStdResult{}, err
		}

		captured[target] = results
	}

	partition := partitionByTarget(captured, targets)

	if err := writeDedupedTypedefs(outDir, partition); err != nil {
		return GenerateStdResult{}, fmt.Errorf("dedup step: %w", err)
	}

	if err := generateRustIndexFile(outDir, partition, targets); err != nil {
		return GenerateStdResult{}, fmt.Errorf("failed to generate Rust index file: %w", err)
	}

	totalGenerated := 0
	for _, pkgs := range captured {
		totalGenerated += len(pkgs)
	}

	return GenerateStdResult{
		Generated: totalGenerated,
		Duration:  time.Since(start),
	}, nil
}

// listStdlibPackages runs `go list std` per target so the list reflects
// platform-conditional packages like `plugin` (absent on windows).
func listStdlibPackages(target Target) ([]string, error) {
	cmd := exec.Command("go", "list", "std")
	cmd.Env = append(os.Environ(),
		"GOOS="+target.GOOS,
		"GOARCH="+target.GOARCH,
		"CGO_ENABLED=0",
	)
	out, err := cmd.Output()
	if err != nil {
		return nil, err
	}

	var pkgs []string
	for line := range strings.SplitSeq(strings.TrimSpace(string(out)), "\n") {
		if shouldSkipPackage(line) {
			continue
		}
		pkgs = append(pkgs, line)
	}

	return pkgs, nil
}

func shouldSkipPackage(pkg string) bool {
	return strings.Contains(pkg, "/internal") ||
		strings.HasPrefix(pkg, "internal/") ||
		strings.Contains(pkg, "/vendor/") ||
		strings.HasPrefix(pkg, "vendor/") ||
		strings.HasSuffix(pkg, "_test")
}

// writeDedupedTypedefs writes the partitioned outputs and removes stale
// files from prior runs. Each divergent package emits one file per content
// variant, named after the variant's canonical target.
func writeDedupedTypedefs(outDir string, partition partitioned) error {
	written := make(map[string]struct{})

	write := func(outPath, content string) error {
		if err := os.MkdirAll(filepath.Dir(outPath), 0755); err != nil {
			return fmt.Errorf("mkdir %s: %w", filepath.Dir(outPath), err)
		}
		if err := os.WriteFile(outPath, []byte(content), 0644); err != nil {
			return fmt.Errorf("write %s: %w", outPath, err)
		}
		written[outPath] = struct{}{}
		return nil
	}

	for pkgPath, content := range partition.common {
		if err := write(filepath.Join(outDir, pkgPath+".d.lis"), content); err != nil {
			return err
		}
	}

	for pkgPath, variants := range partition.variants {
		for _, v := range variants {
			if err := write(filepath.Join(outDir, suffixedPath(pkgPath, v.canonical)), v.content); err != nil {
				return err
			}
		}
	}

	if err := removeStaleTypedefs(outDir, written); err != nil {
		return fmt.Errorf("remove stale: %w", err)
	}

	return nil
}

// suffixedPath converts "os/user" + linux/amd64 into "os/user_linux_amd64.d.lis".
// The suffix attaches to the basename, never to a directory segment.
func suffixedPath(pkgPath string, target Target) string {
	dir, base := filepath.Split(pkgPath)
	return filepath.Join(dir, base+"_"+target.Suffix()+".d.lis")
}

func removeStaleTypedefs(outDir string, kept map[string]struct{}) error {
	return filepath.Walk(outDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}
		if !strings.HasSuffix(path, ".d.lis") {
			return nil
		}
		if _, ok := kept[path]; ok {
			return nil
		}
		return os.Remove(path)
	})
}
