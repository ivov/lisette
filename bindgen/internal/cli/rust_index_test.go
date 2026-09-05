package cli

import (
	"os"
	"path/filepath"
	"testing"
)

func readIndexTestFile(t *testing.T, path string) string {
	t.Helper()
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(content)
}

func TestStdlibIndexPreservesCanonicalFilesAndAvailability(t *testing.T) {
	targets := []Target{{"linux", "amd64"}, {"linux", "arm64"}, {"windows", "amd64"}}
	captured := map[Target]map[string]string{
		targets[0]: {"shared": "struct Shared {}", "limited": "struct Limited {}", "nested/variant": "struct Linux {}"},
		targets[1]: {"shared": "struct Shared {}", "limited": "struct Limited {}", "nested/variant": "struct Linux {}"},
		targets[2]: {"shared": "struct Shared {}", "nested/variant": "struct Windows {}"},
	}
	root := t.TempDir()
	sourceDirectory := filepath.Join(root, "src")
	if err := os.Mkdir(sourceDirectory, 0755); err != nil {
		t.Fatal(err)
	}
	outputDirectory := filepath.Join(root, "typedefs")
	partition := partitionByTarget(captured, targets)
	if err := writeDedupedTypedefs(outputDirectory, partition); err != nil {
		t.Fatal(err)
	}
	if err := generateRustIndexFile(outputDirectory, partition, targets); err != nil {
		t.Fatal(err)
	}
	metadata := readIndexTestFile(t, filepath.Join(sourceDirectory, "go_modules.tsv"))
	expected := "common\tlimited\tlimited.d.lis\n" +
		"targets\tlimited\tlinux\tamd64\tlinux\tarm64\n" +
		"overlay\tlinux\tamd64\tnested/variant\tnested/variant_linux_amd64.d.lis\n" +
		"overlay\tlinux\tarm64\tnested/variant\tnested/variant_linux_amd64.d.lis\n" +
		"overlay\twindows\tamd64\tnested/variant\tnested/variant_windows_amd64.d.lis\n" +
		"common\tshared\tshared.d.lis\n"
	if metadata != expected {
		t.Fatalf("metadata mismatch\ngot: %s\nwant: %s", metadata, expected)
	}
	for filename, expected := range map[string]string{
		"limited.d.lis":                      "struct Limited {}",
		"shared.d.lis":                       "struct Shared {}",
		"nested/variant_linux_amd64.d.lis":   "struct Linux {}",
		"nested/variant_windows_amd64.d.lis": "struct Windows {}",
	} {
		if readIndexTestFile(t, filepath.Join(outputDirectory, filename)) != expected {
			t.Fatalf("typedef bytes changed for %s", filename)
		}
	}
}

func TestStdlibIndexWithoutSourceDirectoryIsSkipped(t *testing.T) {
	root := t.TempDir()
	if err := generateRustIndexFile(filepath.Join(root, "typedefs"), nil, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(root, "src")); !os.IsNotExist(err) {
		t.Fatalf("source directory was created: %v", err)
	}
}
