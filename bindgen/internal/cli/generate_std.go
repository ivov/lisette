package cli

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/convert"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/sync/errgroup"
)

type GenerateStdResult struct {
	Generated int
	Duration  time.Duration
}

type Target struct {
	goos, goarch string
}

func (t Target) String() string {
	return t.goos + "/" + t.goarch
}

func (t Target) Suffix() string {
	return t.goos + "_" + t.goarch
}

// GenerateStd generates per-target `.d.lis` files and deduplicates them
// into a suffixless shared layer plus per-target overlays.
func GenerateStd(ctx context.Context, outDir, lisetteVersion, goVersion string, cfg *config.Config, targets []Target) (GenerateStdResult, error) {
	start := time.Now()

	if len(targets) < 2 {
		return GenerateStdResult{}, fmt.Errorf("at least two targets are required: a single-target regen cannot distinguish platform-conditional packages from common ones")
	}

	captured := make(map[Target]map[string]string)
	var capturedMu sync.Mutex

	targetGroup, targetCtx := errgroup.WithContext(ctx)
	// two measured fastest, more contend on the go build cache
	targetGroup.SetLimit(2)

	for _, target := range targets {
		targetGroup.Go(func() error {
			loadStart := time.Now()
			pkgs, err := extract.LoadStdPackages(target.goos, target.goarch, shouldSkipPackage)
			if err != nil {
				return fmt.Errorf("target %s: failed to load packages: %w", target, err)
			}
			fmt.Fprintf(os.Stderr, "[%s] loaded %d packages in %.1fs\n", target, len(pkgs), time.Since(loadStart).Seconds())

			nilness := convert.NewNilnessAnalysis(pkgs, cfg)

			var generated atomic.Int32
			total := len(pkgs)

			results := make(map[string]string, len(pkgs))
			var resultsMu sync.Mutex

			g, gctx := errgroup.WithContext(targetCtx)
			g.SetLimit(runtime.NumCPU())

			for _, pkg := range pkgs {
				g.Go(func() error {
					select {
					case <-gctx.Done():
						return gctx.Err()
					default:
					}

					result := generateFromPackage(pkg, pkg.PkgPath, lisetteVersion, goVersion, cfg, nilness)

					resultsMu.Lock()
					results[pkg.PkgPath] = result.Content
					resultsMu.Unlock()

					n := generated.Add(1)
					fmt.Fprintf(os.Stderr, "[%s %3d/%d] %s\n", target, n, total, pkg.PkgPath)

					return nil
				})
			}

			if err := g.Wait(); err != nil {
				return err
			}

			capturedMu.Lock()
			captured[target] = results
			capturedMu.Unlock()
			return nil
		})
	}

	if err := targetGroup.Wait(); err != nil {
		return GenerateStdResult{}, err
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

	for pkgPath, pkg := range partition {
		if len(pkg.variants) == 1 {
			if err := write(filepath.Join(outDir, pkgPath+".d.lis"), pkg.variants[0].content); err != nil {
				return err
			}
			continue
		}
		for _, v := range pkg.variants {
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
