package tests

import (
	"bytes"
	"context"
	"flag"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ivov/lisette/bindgen/internal/cli"
	"github.com/ivov/lisette/bindgen/internal/config"
)

var update = flag.Bool("update", false, "update snapshot files")

func TestBindgenSnapshots(t *testing.T) {
	fixtures := findFixtures(t, "testdata/fixtures")

	for _, fixture := range fixtures {
		relPath, err := filepath.Rel("testdata/fixtures", fixture)
		if err != nil {
			t.Fatalf("failed to get relative path: %v", err)
		}

		t.Run(relPath, func(t *testing.T) {
			output := runBindgen(t, fixture)

			snapshotPath := snapshotPathFor(relPath)

			if *update {
				if err := os.MkdirAll(filepath.Dir(snapshotPath), 0755); err != nil {
					t.Fatalf("failed to create snapshot dir: %v", err)
				}
				if err := os.WriteFile(snapshotPath, output, 0644); err != nil {
					t.Fatalf("failed to write snapshot: %v", err)
				}
				return
			}

			expected, err := os.ReadFile(snapshotPath)
			if err != nil {
				t.Fatalf("snapshot not found: %s (run with -update to create)", snapshotPath)
			}

			if diff := diffOutput(expected, output); diff != "" {
				t.Errorf("output mismatch:\n%s", diff)
			}
		})
	}
}

func findFixtures(t *testing.T, root string) []string {
	var fixtures []string

	err := filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			entries, err := os.ReadDir(path)
			if err != nil {
				return err
			}
			for _, entry := range entries {
				if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".go") {
					fixtures = append(fixtures, path)
					break
				}
			}
		}
		return nil
	})

	if err != nil {
		t.Fatalf("failed to walk fixtures: %v", err)
	}

	return fixtures
}

func snapshotPathFor(fixturePath string) string {
	return filepath.Join("testdata/snapshots", fixturePath+".d.lis")
}

func runBindgen(t *testing.T, pkgPath string) []byte {
	var cfg *config.Config
	cfgPath := filepath.Join(pkgPath, "bindgen.json")
	if _, err := os.Stat(cfgPath); err == nil {
		loaded, err := config.LoadConfig(cfgPath, nil)
		if err != nil {
			t.Fatalf("failed to load fixture config %s: %v", cfgPath, err)
		}
		cfg = &loaded
	}

	result, err := cli.GeneratePkg("./"+pkgPath, "0.0.0", "0.0.0", cfg)
	if err != nil {
		t.Fatalf("bindgen failed: %v", err)
	}

	return []byte(result.Content)
}

func TestGenerateError(t *testing.T) {
	_, err := cli.GeneratePkg("/nonexistent/path/to/package", "0.0.0", "0.0.0", nil)
	if err == nil {
		t.Error("expected error for nonexistent package, got nil")
	}
}

func diffOutput(expected, actual []byte) string {
	if bytes.Equal(expected, actual) {
		return ""
	}

	expectedLines := strings.Split(string(expected), "\n")
	actualLines := strings.Split(string(actual), "\n")

	var diff strings.Builder
	maxLines := len(expectedLines)
	if len(actualLines) > maxLines {
		maxLines = len(actualLines)
	}

	for i := 0; i < maxLines; i++ {
		var exp, act string
		if i < len(expectedLines) {
			exp = expectedLines[i]
		}
		if i < len(actualLines) {
			act = actualLines[i]
		}
		if exp != act {
			diff.WriteString("--- expected ---\n")
			diff.WriteString(exp)
			diff.WriteString("\n+++ actual +++\n")
			diff.WriteString(act)
			diff.WriteString("\n")
		}
	}

	return diff.String()
}

func TestGenerateStd(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stdlib generation test in short mode")
	}

	tmpDir, err := os.MkdirTemp("", "bindgen-stdlib-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer func() { _ = os.RemoveAll(tmpDir) }()

	result, err := cli.GenerateStd(context.Background(), tmpDir, "0.0.0", "0.0.0", nil)
	if err != nil {
		t.Fatalf("GenerateStd failed: %v", err)
	}

	if result.Generated == 0 {
		t.Error("expected to generate at least one package")
	}

	expectedFiles := []string{
		"fmt.d.lis",
		"os.d.lis",
		"net/http.d.lis",
	}
	for _, f := range expectedFiles {
		path := filepath.Join(tmpDir, f)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Errorf("expected file %s not found", f)
		}
	}
}
