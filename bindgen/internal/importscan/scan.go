// Package importscan applies the file and directory rules of `go mod tidy` to list imports.
package importscan

import (
	"bytes"
	"fmt"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"golang.org/x/mod/modfile"
)

var byteOrderMark = []byte("\xef\xbb\xbf")

type Package struct {
	Path    string   `json:"path"`
	Imports []string `json:"imports,omitempty"`
	Error   string   `json:"error,omitempty"`
}

// Scan returns the package at dir, and false if no file in dir builds for any target.
func Scan(path, dir string) (Package, bool) {
	imports, found, err := scanDir(dir)
	if err != nil {
		return Package{Path: path, Error: err.Error()}, true
	}
	return Package{Path: path, Imports: imports}, found
}

func scanDir(dir string) ([]string, bool, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, false, err
	}

	imports := make(map[string]bool)
	found := false
	for _, entry := range entries {
		name := entry.Name()
		if !isSourceFile(name) || !isRegularFile(dir, entry) {
			continue
		}
		file := filepath.Join(dir, name)
		content, err := os.ReadFile(file)
		if err != nil {
			return nil, false, err
		}
		content = bytes.TrimPrefix(content, byteOrderMark)
		if !matchesAnyTags(content) {
			continue
		}
		parsed, err := parser.ParseFile(token.NewFileSet(), file, content, parser.ImportsOnly)
		if err != nil {
			return nil, false, fmt.Errorf("reading %s: %v", file, err)
		}
		found = true
		for _, spec := range parsed.Imports {
			imported, err := strconv.Unquote(spec.Path.Value)
			if err != nil || isImplicitImport(imported) {
				continue
			}
			imports[imported] = true
		}
	}

	list := make([]string, 0, len(imports))
	for imported := range imports {
		list = append(list, imported)
	}
	sort.Strings(list)
	return list, found, nil
}

// ModulePackages scans every package in the module at root that some target can build.
func ModulePackages(modulePath, root string) ([]Package, error) {
	ignore, err := ignorePatterns(filepath.Join(root, "go.mod"))
	if err != nil {
		return nil, err
	}

	var packages []Package
	err = filepath.WalkDir(root, func(dir string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !entry.IsDir() {
			return nil
		}
		relative, err := filepath.Rel(root, dir)
		if err != nil {
			return err
		}
		relative = filepath.ToSlash(relative)
		if relative == "." {
			relative = ""
		} else {
			name := entry.Name()
			if strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_") || name == "testdata" {
				return filepath.SkipDir
			}
			if ignore.matches(relative) {
				return filepath.SkipDir
			}
			if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
				return filepath.SkipDir
			}
		}

		if pkg, found := Scan(path.Join(modulePath, relative), dir); found {
			packages = append(packages, pkg)
		}
		if entry.Name() == "vendor" && relative != "" {
			return filepath.SkipDir
		}
		return nil
	})
	return packages, err
}

func isSourceFile(name string) bool {
	return strings.HasSuffix(name, ".go") &&
		!strings.HasSuffix(name, "_test.go") &&
		!strings.HasPrefix(name, "_") &&
		!strings.HasPrefix(name, ".")
}

func isRegularFile(dir string, entry fs.DirEntry) bool {
	if entry.Type()&fs.ModeSymlink == 0 {
		return entry.Type().IsRegular()
	}
	info, err := os.Stat(filepath.Join(dir, entry.Name()))
	return err == nil && info.Mode().IsRegular()
}

func isImplicitImport(imported string) bool {
	return imported == "C" ||
		imported == "appengine" || strings.HasPrefix(imported, "appengine/") ||
		imported == "appengine_internal" || strings.HasPrefix(imported, "appengine_internal/")
}

type ignoreList struct {
	rooted   []string
	anywhere []string
}

func ignorePatterns(goModPath string) (ignoreList, error) {
	var list ignoreList
	content, err := os.ReadFile(goModPath)
	if os.IsNotExist(err) {
		return list, nil
	}
	if err != nil {
		return list, err
	}
	parsed, err := modfile.ParseLax(goModPath, content, nil)
	if err != nil {
		return list, err
	}
	for _, directive := range parsed.Ignore {
		pattern, rooted := strings.CutPrefix(directive.Path, "./")
		pattern = enclosedInSlashes(pattern)
		if rooted {
			list.rooted = append(list.rooted, pattern)
		} else {
			list.anywhere = append(list.anywhere, pattern)
		}
	}
	return list, nil
}

func (list ignoreList) matches(relative string) bool {
	dir := enclosedInSlashes(relative)
	for _, pattern := range list.rooted {
		if strings.HasPrefix(dir, pattern) {
			return true
		}
	}
	for _, pattern := range list.anywhere {
		if strings.Contains(dir, pattern) {
			return true
		}
	}
	return false
}

func enclosedInSlashes(value string) string {
	value = filepath.ToSlash(value)
	if !strings.HasPrefix(value, "/") {
		value = "/" + value
	}
	if !strings.HasSuffix(value, "/") {
		value += "/"
	}
	return value
}
