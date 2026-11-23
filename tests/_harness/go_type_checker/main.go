// Command go_type_checker type-checks every package of a Go module with go/types,
// without compiling or linking. Imports from outside the module come from the
// export data that `go list -export` writes to the build cache.
// Usage: go_type_checker <module dir>
// Output: one line per error, "<package dir>\t<message>".
package main

import (
	"bufio"
	"bytes"
	"fmt"
	"go/ast"
	"go/importer"
	"go/parser"
	"go/token"
	"go/types"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type localPackage struct {
	dir     string
	files   []*ast.File
	types   *types.Package
	errors  []string
	checked bool
}

type checker struct {
	root       string
	fset       *token.FileSet
	modulePath string
	goVersion  string
	local      map[string]*localPackage
	exports    map[string]string
	external   types.Importer
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: go_type_checker <module dir>")
		os.Exit(2)
	}
	root, err := filepath.Abs(os.Args[1])
	if err != nil {
		fatal(err)
	}
	c := &checker{
		root:    root,
		fset:    token.NewFileSet(),
		local:   map[string]*localPackage{},
		exports: map[string]string{},
	}
	if err := c.readGoMod(); err != nil {
		fatal(err)
	}
	if err := c.parsePackages(); err != nil {
		fatal(err)
	}
	if err := c.loadExports(); err != nil {
		fatal(err)
	}
	c.external = importer.ForCompiler(c.fset, "gc", func(path string) (io.ReadCloser, error) {
		file := c.exports[path]
		if file == "" {
			return nil, fmt.Errorf("no export data for %q", path)
		}
		return os.Open(file)
	})
	paths := make([]string, 0, len(c.local))
	for path := range c.local {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	for _, path := range paths {
		pkg := c.check(path)
		for _, line := range pkg.errors {
			fmt.Fprintf(out, "%s\t%s\n", pkg.dir, strings.ReplaceAll(line, "\n", " "))
		}
	}
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(2)
}

func (c *checker) readGoMod() error {
	content, err := os.ReadFile(filepath.Join(c.root, "go.mod"))
	if err != nil {
		return err
	}
	for _, line := range strings.Split(string(content), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 2 && fields[0] == "module" {
			c.modulePath = fields[1]
		}
		if len(fields) == 2 && fields[0] == "go" {
			c.goVersion = "go" + fields[1]
		}
	}
	if c.modulePath == "" {
		return fmt.Errorf("no module line in %s", filepath.Join(c.root, "go.mod"))
	}
	return nil
}

func (c *checker) parsePackages() error {
	return filepath.WalkDir(c.root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		dir, err := filepath.Rel(c.root, filepath.Dir(path))
		if err != nil {
			return err
		}
		importPath := c.modulePath
		if dir != "." {
			importPath += "/" + filepath.ToSlash(dir)
		}
		pkg := c.local[importPath]
		if pkg == nil {
			pkg = &localPackage{dir: filepath.ToSlash(dir)}
			c.local[importPath] = pkg
		}
		file, err := parser.ParseFile(c.fset, path, nil, parser.SkipObjectResolution)
		if err != nil {
			pkg.errors = append(pkg.errors, c.relative(err.Error()))
		}
		if file != nil {
			pkg.files = append(pkg.files, file)
		}
		return nil
	})
}

func (c *checker) loadExports() error {
	imports := map[string]bool{}
	for _, pkg := range c.local {
		for _, file := range pkg.files {
			for _, spec := range file.Imports {
				path := strings.Trim(spec.Path.Value, "\"`")
				if _, isLocal := c.local[path]; !isLocal {
					imports[path] = true
				}
			}
		}
	}
	if len(imports) == 0 {
		return nil
	}
	args := []string{"list", "-e", "-export", "-deps", "-f", "{{.ImportPath}}\t{{.Export}}"}
	for path := range imports {
		args = append(args, path)
	}
	command := exec.Command("go", args...)
	command.Dir = c.root
	var stderr bytes.Buffer
	command.Stderr = &stderr
	output, err := command.Output()
	if err != nil {
		return fmt.Errorf("go list: %v\n%s", err, stderr.String())
	}
	for _, line := range strings.Split(string(output), "\n") {
		path, file, found := strings.Cut(line, "\t")
		if found {
			c.exports[path] = file
		}
	}
	return nil
}

func (c *checker) Import(path string) (*types.Package, error) {
	if _, isLocal := c.local[path]; isLocal {
		pkg := c.check(path)
		if pkg.types == nil {
			return nil, fmt.Errorf("package %s does not type-check", path)
		}
		return pkg.types, nil
	}
	return c.external.Import(path)
}

func (c *checker) check(path string) *localPackage {
	pkg := c.local[path]
	if pkg.checked {
		return pkg
	}
	pkg.checked = true
	config := types.Config{
		GoVersion: c.goVersion,
		Importer:  c,
		Error: func(err error) {
			pkg.errors = append(pkg.errors, c.relative(err.Error()))
		},
	}
	pkg.types, _ = config.Check(path, c.fset, pkg.files, nil)
	return pkg
}

func (c *checker) relative(message string) string {
	return strings.ReplaceAll(message, c.root+string(filepath.Separator), "")
}
