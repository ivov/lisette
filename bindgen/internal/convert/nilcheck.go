package convert

import (
	"go/ast"
	"go/token"
	"go/types"
)

// Package-level variable nilability. Return nilability lives in nilness.go.

func (c *Converter) ensureFuncDeclCache() {
	if c.funcDeclCache != nil {
		return
	}
	c.funcDeclCache = make(map[token.Pos]*ast.FuncDecl)
	if c.pkg == nil {
		return
	}
	for _, file := range c.pkg.Syntax {
		for _, decl := range file.Decls {
			fn, ok := decl.(*ast.FuncDecl)
			if ok && fn.Body != nil {
				c.funcDeclCache[fn.Name.Pos()] = fn
			}
		}
	}
}

func (c *Converter) findFuncDecl(obj types.Object) *ast.FuncDecl {
	c.ensureFuncDeclCache()
	return c.funcDeclCache[obj.Pos()]
}

// isProvenNonNilVar checks if a package-level variable is proven non-nil.
func (c *Converter) isProvenNonNilVar(obj types.Object) bool {
	if c.pkg == nil {
		return false
	}

	entry, ok := c.valueSpecFor(obj)
	if !ok || entry.tok != token.VAR {
		return c.isAssignedNonNilInInit(obj)
	}
	if entry.index < len(entry.spec.Values) {
		return c.isProvenNonNilExprSimple(entry.spec.Values[entry.index])
	}

	// No initializer (var X *T) — check init() functions
	return c.isAssignedNonNilInInit(obj)
}

type valueSpecEntry struct {
	spec  *ast.ValueSpec
	index int // position of the object's name within spec.Names
	tok   token.Token
}

// valueSpecFor resolves a const or var object to its declaring ValueSpec.
func (c *Converter) valueSpecFor(obj types.Object) (valueSpecEntry, bool) {
	pos := obj.Pos()
	if !pos.IsValid() || c.pkg == nil {
		return valueSpecEntry{}, false
	}
	if c.valueSpecIndex == nil {
		c.valueSpecIndex = make(map[token.Pos]valueSpecEntry)
		for _, file := range c.pkg.Syntax {
			if file == nil {
				continue
			}
			for _, decl := range file.Decls {
				genDecl, ok := decl.(*ast.GenDecl)
				if !ok || (genDecl.Tok != token.VAR && genDecl.Tok != token.CONST) {
					continue
				}
				for _, spec := range genDecl.Specs {
					vs, ok := spec.(*ast.ValueSpec)
					if !ok {
						continue
					}
					for i, name := range vs.Names {
						c.valueSpecIndex[name.Pos()] = valueSpecEntry{spec: vs, index: i, tok: genDecl.Tok}
					}
				}
			}
		}
	}
	entry, ok := c.valueSpecIndex[pos]
	return entry, ok
}

// isProvenNonNilExprSimple: callee nilness facts outrank constructor names.
func (c *Converter) isProvenNonNilExprSimple(expr ast.Expr) bool {
	if unary, ok := expr.(*ast.UnaryExpr); ok && unary.Op == token.AND {
		return true
	}
	if _, ok := expr.(*ast.CompositeLit); ok {
		return true
	}
	if sel, ok := expr.(*ast.SelectorExpr); ok && c.pkg != nil && c.pkg.TypesInfo != nil {
		if obj, ok := c.pkg.TypesInfo.Uses[sel.Sel]; ok {
			if _, isConst := obj.(*types.Const); isConst {
				return true
			}
		}
	}
	call, ok := expr.(*ast.CallExpr)
	if !ok {
		return false
	}

	var calleeName string
	var calleeObj types.Object
	switch fn := call.Fun.(type) {
	case *ast.Ident:
		calleeName = fn.Name
		if c.pkg != nil && c.pkg.TypesInfo != nil {
			calleeObj = c.pkg.TypesInfo.Uses[fn]
		}
	case *ast.SelectorExpr:
		calleeName = fn.Sel.Name
		if c.pkg != nil && c.pkg.TypesInfo != nil {
			calleeObj = c.pkg.TypesInfo.Uses[fn.Sel]
		}
	default:
		return false
	}

	if calleeName == "new" || calleeName == "make" {
		return true
	}
	if calleeObj != nil {
		if facts, ok := c.nilness.Function(calleeObj); ok && facts.HasBody {
			switch facts.Single {
			case ReturnProvenNonNil:
				return true
			case ReturnHasNilPath:
				return false
			}
		}
	}
	return looksLikeConstructor(calleeName)
}

// isAssignedNonNilInInit checks if a package-level variable is assigned a non-nil value in an init() function.
func (c *Converter) isAssignedNonNilInInit(obj types.Object) bool {
	if c.pkg == nil || c.pkg.TypesInfo == nil {
		return false
	}
	for _, file := range c.pkg.Syntax {
		for _, decl := range file.Decls {
			fn, ok := decl.(*ast.FuncDecl)
			if !ok || fn.Name.Name != "init" || fn.Recv != nil || fn.Body == nil {
				continue
			}
			found := false
			ast.Inspect(fn.Body, func(n ast.Node) bool {
				assign, ok := n.(*ast.AssignStmt)
				if !ok {
					return true
				}
				for i, lhs := range assign.Lhs {
					lhsIdent, ok := lhs.(*ast.Ident)
					if !ok {
						continue
					}
					lhsObj := c.pkg.TypesInfo.Uses[lhsIdent]
					if lhsObj == nil {
						lhsObj = c.pkg.TypesInfo.ObjectOf(lhsIdent)
					}
					if lhsObj == obj && i < len(assign.Rhs) {
						if c.isProvenNonNilExprSimple(assign.Rhs[i]) {
							found = true
							return false
						}
					}
				}
				return true
			})
			if found {
				return true
			}
		}
	}
	return false
}

func ncGetReceiverName(fn *ast.FuncDecl) string {
	if fn.Recv == nil || len(fn.Recv.List) == 0 {
		return ""
	}
	field := fn.Recv.List[0]
	if len(field.Names) > 0 {
		return field.Names[0].Name
	}
	return ""
}
