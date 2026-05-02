package extract

import (
	"fmt"
	"go/doc"
	"go/types"
	"os"
	"sort"
	"strings"

	"golang.org/x/tools/go/packages"
)

type SymbolExportKind int

const (
	ExportFunction SymbolExportKind = iota
	ExportType
	ExportConstant
	ExportMethod
	ExportVariable
)

type SymbolExport struct {
	Name                 string
	Kind                 SymbolExportKind
	Doc                  string
	GoType               types.Type
	Obj                  types.Object
	ReceiverVariable     *types.Var
	BaseType             *types.Named // for methods
	IsPromoted           bool         // true if promoted from an embedded field
	NeedsPointerReceiver bool         // for promoted methods: true if only in pointer method set
	OriginalTypeName     string       // for promoted methods: declaring type name
	OriginalPkgPath      string       // for promoted methods: declaring type's package path
}

var loadConfig = &packages.Config{
	Mode: packages.NeedName |
		packages.NeedTypes |
		packages.NeedTypesInfo |
		packages.NeedSyntax |
		packages.NeedDeps |
		packages.NeedImports,
	Env: buildLoaderEnv(),
}

func buildLoaderEnv() []string {
	targetGOOS := os.Getenv("BINDGEN_TARGET_GOOS")
	targetGOARCH := os.Getenv("BINDGEN_TARGET_GOARCH")

	env := os.Environ()
	if targetGOOS != "" || targetGOARCH != "" {
		filtered := make([]string, 0, len(env))
		for _, e := range env {
			if (targetGOOS != "" && strings.HasPrefix(e, "GOOS=")) ||
				(targetGOARCH != "" && strings.HasPrefix(e, "GOARCH=")) {
				continue
			}
			filtered = append(filtered, e)
		}
		env = filtered
		if targetGOOS != "" {
			env = append(env, "GOOS="+targetGOOS)
		}
		if targetGOARCH != "" {
			env = append(env, "GOARCH="+targetGOARCH)
		}
	}

	return append(env, "CGO_ENABLED=0", "GOFLAGS=-mod=mod")
}

func LoadPackage(path string) (*packages.Package, error) {
	pkgs, err := packages.Load(loadConfig, path)
	if err != nil {
		return nil, err
	}

	if len(pkgs) == 0 {
		return nil, nil
	}

	if len(pkgs) > 1 {
		pkgs = pkgs[:1]
	}

	pkg := pkgs[0]

	for _, pkgErr := range pkg.Errors {
		if pkgErr.Kind == packages.ListError || pkgErr.Kind == packages.UnknownError {
			return nil, fmt.Errorf("failed to load package: %v", pkgErr)
		}
	}

	return pkg, nil
}

func LoadPackages(paths []string) ([]*packages.Package, error) {
	pkgs, err := packages.Load(loadConfig, paths...)
	if err != nil {
		return nil, err
	}

	var result []*packages.Package
	for _, pkg := range pkgs {
		if len(pkg.Errors) > 0 {
			continue
		}
		result = append(result, pkg)
	}

	return result, nil
}

// Like LoadPackages but keeps errored packages so the caller can classify them.
func LoadPackagesAll(paths []string) ([]*packages.Package, error) {
	return packages.Load(loadConfig, paths...)
}

func ExtractExports(pkg *packages.Package) []SymbolExport {
	if pkg == nil || pkg.Types == nil {
		return nil
	}

	var exports []SymbolExport

	docPkg := buildDocPackage(pkg)

	pkgScope := pkg.Types.Scope()
	pkgNames := pkgScope.Names()

	for _, name := range pkgNames {
		obj := pkgScope.Lookup(name)
		if obj == nil || !obj.Exported() {
			continue
		}

		doc := getDocForObject(docPkg, name)

		switch o := obj.(type) {
		case *types.Func:
			exports = append(exports, SymbolExport{
				Name:   name,
				Kind:   ExportFunction,
				Doc:    doc,
				GoType: o.Type(),
				Obj:    o,
			})

		case *types.TypeName:
			exports = append(exports, SymbolExport{
				Name:   name,
				Kind:   ExportType,
				Doc:    doc,
				GoType: o.Type(),
				Obj:    o,
			})

			if named, ok := o.Type().(*types.Named); ok {
				methodExports := extractMethods(named, pkg, docPkg)
				exports = append(exports, methodExports...)
			}

		case *types.Const:
			exports = append(exports, SymbolExport{
				Name:   name,
				Kind:   ExportConstant,
				Doc:    doc,
				GoType: o.Type(),
				Obj:    o,
			})

		case *types.Var:
			exports = append(exports, SymbolExport{
				Name:   name,
				Kind:   ExportVariable,
				Doc:    doc,
				GoType: o.Type(),
				Obj:    o,
			})
		}
	}

	sort.Slice(exports, func(i, j int) bool {
		if exports[i].Kind != exports[j].Kind {
			return exports[i].Kind < exports[j].Kind
		}
		return exports[i].Name < exports[j].Name
	})

	return exports
}

func extractMethods(named *types.Named, pkg *packages.Package, docPkg *doc.Package) []SymbolExport {
	var exports []SymbolExport

	declaredMethods := make(map[string]bool)
	for method := range named.Methods() {
		declaredMethods[method.Name()] = true
	}

	valMethodSet := types.NewMethodSet(named)
	ptrMethodSet := types.NewMethodSet(types.NewPointer(named))

	docPkgCache := map[string]*doc.Package{pkg.PkgPath: docPkg}

	for sel := range ptrMethodSet.Methods() {
		methodObj := sel.Obj()

		if !methodObj.Exported() {
			continue
		}

		fn, ok := methodObj.(*types.Func)
		if !ok {
			continue
		}

		isPromoted := !declaredMethods[methodObj.Name()]
		needsPointerReceiver := valMethodSet.Lookup(methodObj.Pkg(), methodObj.Name()) == nil

		sig := fn.Type().(*types.Signature)
		recv := sig.Recv()

		lookupDocPkg := docPkg
		docTypeName := named.Obj().Name()
		var originalTypeName, originalPkgPath string
		if isPromoted && recv != nil {
			t := recv.Type()
			if ptr, ok := t.(*types.Pointer); ok {
				t = ptr.Elem()
			}
			if n, ok := t.(*types.Named); ok {
				docTypeName = n.Obj().Name()
				originalTypeName = n.Obj().Name()
				if objPkg := n.Obj().Pkg(); objPkg != nil {
					originalPkgPath = objPkg.Path()
					if objPkg.Path() != pkg.PkgPath {
						lookupDocPkg = resolveDocPkg(docPkgCache, pkg, objPkg.Path())
					}
				}
			}
		}

		methodDoc := getMethodDoc(lookupDocPkg, docTypeName, methodObj.Name())

		exports = append(exports, SymbolExport{
			Name:                 methodObj.Name(),
			Kind:                 ExportMethod,
			Doc:                  methodDoc,
			GoType:               fn.Type(),
			Obj:                  fn,
			ReceiverVariable:     recv,
			BaseType:             named,
			IsPromoted:           isPromoted,
			NeedsPointerReceiver: needsPointerReceiver,
			OriginalTypeName:     originalTypeName,
			OriginalPkgPath:      originalPkgPath,
		})
	}

	return exports
}

func resolveDocPkg(cache map[string]*doc.Package, pkg *packages.Package, path string) *doc.Package {
	if cached, ok := cache[path]; ok {
		return cached
	}
	if importedPkg, ok := pkg.Imports[path]; ok {
		dp := buildDocPackage(importedPkg)
		cache[path] = dp
		return dp
	}
	return nil
}
