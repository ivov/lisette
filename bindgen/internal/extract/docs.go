package extract

import (
	"go/ast"
	"go/doc"
	"slices"
	"strings"

	"golang.org/x/tools/go/packages"
)

func buildDocPackage(pkg *packages.Package) *doc.Package {
	if len(pkg.Syntax) == 0 {
		return nil
	}

	docPkg, err := doc.NewFromFiles(pkg.Fset, pkg.Syntax, pkg.PkgPath, doc.AllDecls|doc.PreserveAST)
	if err != nil {
		return nil
	}
	return docPkg
}

func getDocForObject(docPkg *doc.Package, name string) string {
	if docPkg == nil {
		return ""
	}

	for _, f := range docPkg.Funcs {
		if f.Name == name {
			return cleanDoc(f.Doc)
		}
	}

	for _, t := range docPkg.Types {
		if t.Name == name {
			return cleanDoc(t.Doc)
		}
	}

	for _, c := range docPkg.Consts {
		if slices.Contains(c.Names, name) {
			return cleanDoc(c.Doc)
		}
	}

	for _, v := range docPkg.Vars {
		if slices.Contains(v.Names, name) {
			return cleanDoc(v.Doc)
		}
	}

	return ""
}

func getMethodDoc(docPkg *doc.Package, typeName, methodName string) string {
	if docPkg == nil {
		return ""
	}

	for _, t := range docPkg.Types {
		if t.Name != typeName {
			continue
		}

		for _, m := range t.Methods {
			if m.Name == methodName {
				return cleanDoc(m.Doc)
			}
		}

		// For interface methods, docs are in the AST, not in t.Methods
		if t.Decl != nil {
			for _, spec := range t.Decl.Specs {
				ts, ok := spec.(*ast.TypeSpec)
				if !ok {
					continue
				}
				iface, ok := ts.Type.(*ast.InterfaceType)
				if !ok || iface.Methods == nil {
					continue
				}
				for _, field := range iface.Methods.List {
					if len(field.Names) == 1 && field.Names[0].Name == methodName && field.Doc != nil {
						return cleanDoc(field.Doc.Text())
					}
				}
			}
		}
	}

	return ""
}

func cleanDoc(doc string) string {
	doc = strings.TrimSpace(doc)
	doc = strings.TrimRight(doc, "\n")
	return doc
}
