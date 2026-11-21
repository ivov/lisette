package importscan

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestMatchesAnyTags(t *testing.T) {
	cases := []struct {
		name   string
		header string
		want   bool
	}{
		{"no constraint", "package a\n", true},
		{"target tag", "//go:build windows\n\npackage a\n", true},
		{"custom tag", "//go:build purego\n\npackage a\n", true},
		{"negated tag", "//go:build !purego\n\npackage a\n", true},
		{"tag and its negation", "//go:build purego && !purego\n\npackage a\n", true},
		{"ignore", "//go:build ignore\n\npackage a\n", false},
		{"negated ignore", "//go:build !ignore\n\npackage a\n", true},
		{"ignore or tag", "//go:build ignore || purego\n\npackage a\n", true},
		{"ignore and tag", "//go:build ignore && purego\n\npackage a\n", false},
		{"malformed", "//go:build (purego\n\npackage a\n", false},
		{"two go:build lines", "//go:build purego\n//go:build windows\n\npackage a\n", false},
		{"plus build ignore", "// +build ignore\n\npackage a\n", false},
		{"plus build tag", "// +build purego\n\npackage a\n", true},
		{"go:build wins over plus build", "//go:build purego\n// +build ignore\n\npackage a\n", true},
		{"constraint after package clause", "package a\n\n//go:build ignore\n", true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := matchesAnyTags([]byte(c.header)); got != c.want {
				t.Errorf("matchesAnyTags(%q) = %v, want %v", c.header, got, c.want)
			}
		})
	}
}

func TestScan(t *testing.T) {
	dir := t.TempDir()
	write(t, dir, "a.go", "package a\n\nimport \"example.com/host\"\n")
	write(t, dir, "a_windows.go", "package a\n\nimport \"example.com/windows\"\n")
	write(t, dir, "a_purego.go", "//go:build purego\n\npackage a\n\nimport \"example.com/purego\"\n")
	write(t, dir, "gen.go", "//go:build ignore\n\npackage main\n\nimport \"example.com/ignored\"\n")
	write(t, dir, "bom.go", "\xef\xbb\xbf//go:build ignore\n\npackage main\n\nimport \"example.com/bom\"\n")
	write(t, dir, "a_test.go", "package a\n\nimport \"example.com/test\"\n")
	write(t, dir, "_hidden.go", "package a\n\nimport \"example.com/hidden\"\n")
	write(t, dir, "cgo.go", "package a\n\nimport \"C\"\nimport \"example.com/cgo\"\n")

	pkg, found := Scan("example.com/a", dir)
	if pkg.Error != "" || !found {
		t.Fatalf("Scan: found=%v error=%q", found, pkg.Error)
	}
	want := []string{"example.com/cgo", "example.com/host", "example.com/purego", "example.com/windows"}
	if !slices.Equal(pkg.Imports, want) {
		t.Errorf("imports = %v, want %v", pkg.Imports, want)
	}
}

func TestScanWithOnlyIgnoredFiles(t *testing.T) {
	dir := t.TempDir()
	write(t, dir, "gen.go", "//go:build ignore\n\npackage main\n")
	write(t, dir, "a_test.go", "package a\n")

	if pkg, found := Scan("example.com/a", dir); pkg.Error != "" || found {
		t.Errorf("Scan: found=%v error=%q, want no package", found, pkg.Error)
	}
}

func TestModulePackages(t *testing.T) {
	root := t.TempDir()
	write(t, root, "go.mod", "module example.com/m\n\ngo 1.25\n\nignore ./skipped\nignore anywhere\n")
	write(t, root, "m.go", "package m\n")
	write(t, root, "win/win_windows.go", "package win\n")
	write(t, root, "only_ignore/gen.go", "//go:build ignore\n\npackage main\n")
	write(t, root, "testdata/t/t.go", "package t\n")
	write(t, root, "_private/p.go", "package p\n")
	write(t, root, ".hidden/h.go", "package h\n")
	write(t, root, "nested/go.mod", "module example.com/m/nested\n")
	write(t, root, "nested/n.go", "package nested\n")
	write(t, root, "skipped/s.go", "package skipped\n")
	write(t, root, "deep/anywhere/x.go", "package x\n")
	write(t, root, "vendor/v.go", "package vendor\n")
	write(t, root, "vendor/below/b.go", "package below\n")

	packages, err := ModulePackages("example.com/m", root)
	if err != nil {
		t.Fatal(err)
	}
	var paths []string
	for _, pkg := range packages {
		paths = append(paths, pkg.Path)
	}
	slices.Sort(paths)
	want := []string{"example.com/m", "example.com/m/vendor", "example.com/m/win"}
	if !slices.Equal(paths, want) {
		t.Errorf("packages = %v, want %v", paths, want)
	}
}

func write(t *testing.T, root, name, content string) {
	t.Helper()
	path := filepath.Join(root, name)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
