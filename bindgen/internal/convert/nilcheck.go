package convert

import (
	"go/ast"
	"go/token"
	"go/types"
)

// nilCacheResult represents the cached result of nilability analysis.
type nilCacheResult int8

const (
	nilCacheInProgress nilCacheResult = -1 // cycle detection: analysis in progress
	nilCacheNotProven  nilCacheResult = 0  // analysis complete: cannot prove non-nil
	nilCacheProven     nilCacheResult = 1  // analysis complete: proven non-nil
)

// isProvenNonNilReturn returns true if AST analysis of the function body proves
// that the function never returns nil from its single pointer return position.
// Called only when isSinglePointerResult is true and no other heuristic has fired.
func (c *Converter) isProvenNonNilReturn(obj types.Object) bool {
	if c.pkg == nil {
		return false
	}

	fn := c.findFuncDecl(obj)
	if fn == nil || fn.Body == nil {
		return false
	}

	return c.analyzeReturnNilability(fn)
}

// isProvenNonNilVar returns true if AST analysis proves the package-level
// variable is always initialized to a non-nil value (in its declaration or init()).
func (c *Converter) isProvenNonNilVar(obj types.Object) bool {
	if c.pkg == nil {
		return false
	}

	spec := c.findVarSpec(obj)
	if spec == nil {
		return c.isAssignedNonNilInInit(obj)
	}

	idx := -1
	for i, name := range spec.Names {
		if name.Pos() == obj.Pos() {
			idx = i
			break
		}
	}
	if idx == -1 {
		return false
	}

	if idx < len(spec.Values) {
		return c.isProvenNonNilExprSimple(spec.Values[idx])
	}

	// No initializer (var X *T) — check init() functions
	return c.isAssignedNonNilInInit(obj)
}

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

func (c *Converter) findVarSpec(obj types.Object) *ast.ValueSpec {
	pos := obj.Pos()
	if !pos.IsValid() || c.pkg == nil {
		return nil
	}
	for _, file := range c.pkg.Syntax {
		for _, decl := range file.Decls {
			genDecl, ok := decl.(*ast.GenDecl)
			if !ok || genDecl.Tok != token.VAR {
				continue
			}
			for _, spec := range genDecl.Specs {
				vs, ok := spec.(*ast.ValueSpec)
				if !ok {
					continue
				}
				for _, name := range vs.Names {
					if name.Pos() == pos {
						return vs
					}
				}
			}
		}
	}
	return nil
}

// analyzeReturnNilability returns true if every return in the function body
// is provably non-nil at the single pointer return position.
func (c *Converter) analyzeReturnNilability(fn *ast.FuncDecl) bool {
	if fn.Body == nil {
		return false
	}

	receiverName := ncGetReceiverName(fn)

	// Naked returns with named pointer results default to nil
	hasNamedPtrResult := false
	if fn.Type.Results != nil {
		for _, field := range fn.Type.Results.List {
			if len(field.Names) > 0 {
				if _, ok := field.Type.(*ast.StarExpr); ok {
					hasNamedPtrResult = true
				}
			}
		}
	}

	proven := true
	ast.Inspect(fn.Body, func(n ast.Node) bool {
		if !proven {
			return false
		}

		// Nested function literals' returns don't apply to the outer function.
		if _, ok := n.(*ast.FuncLit); ok {
			return false
		}

		ret, ok := n.(*ast.ReturnStmt)
		if !ok {
			return true
		}

		if len(ret.Results) == 0 {
			if hasNamedPtrResult {
				proven = false
			}
			return true
		}

		expr := ret.Results[0]

		if ncIsNilExpr(expr) {
			if receiverName != "" && ncIsInsideNilReceiverGuard(fn.Body, ret, receiverName) {
				return true // nil-receiver guard: unreachable in Lisette
			}
			proven = false
			return false
		}

		if !c.isProvenNonNilExpr(expr, fn) {
			proven = false
			return false
		}

		return true
	})

	return proven
}

// isProvenNonNilExpr checks whether an expression is provably non-nil.
func (c *Converter) isProvenNonNilExpr(expr ast.Expr, fn *ast.FuncDecl) bool {
	// &T{}
	if ncIsAddressOfLiteral(expr) {
		return true
	}

	// Variable: receiver-return (technique 2) or def-use (technique 3)
	if ident, ok := expr.(*ast.Ident); ok {
		return c.isProvenNonNilIdent(ident, fn)
	}

	// Call: builtins, constructors, config, transitive (technique 4)
	if call, ok := expr.(*ast.CallExpr); ok {
		// Type conversions look like calls in the AST but preserve nil-ness
		if c.pkg.TypesInfo != nil {
			if tv, ok := c.pkg.TypesInfo.Types[call.Fun]; ok && tv.IsType() {
				if len(call.Args) == 1 {
					return c.isProvenNonNilExpr(call.Args[0], fn)
				}
				return false
			}
		}
		return c.isProvenNonNilCallResult(call)
	}

	// &expr (address-of non-literal, e.g., &localVar)
	if unary, ok := expr.(*ast.UnaryExpr); ok && unary.Op == token.AND {
		return true // &anything is always non-nil
	}

	// Field access, type assertion, index, etc. — can't prove
	return false
}

// isProvenNonNilExprSimple checks if an expression is provably non-nil without
// recursive body analysis. Used for variable initializers and init() assignments.
func (c *Converter) isProvenNonNilExprSimple(expr ast.Expr) bool {
	if ncIsAddressOfLiteral(expr) {
		return true
	}
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
	switch fn := call.Fun.(type) {
	case *ast.Ident:
		if fn.Name == "new" || fn.Name == "make" {
			return true
		}
		return looksLikeConstructor(fn.Name)
	case *ast.SelectorExpr:
		return looksLikeConstructor(fn.Sel.Name)
	}
	return false
}

// isProvenNonNilIdent checks if an identifier is provably non-nil.
func (c *Converter) isProvenNonNilIdent(ident *ast.Ident, fn *ast.FuncDecl) bool {
	if c.pkg.TypesInfo == nil {
		return false
	}

	obj := c.pkg.TypesInfo.ObjectOf(ident)
	if obj == nil {
		return false
	}

	// Receiver-return: method returning its own receiver is guaranteed non-nil
	if fn.Recv != nil && len(fn.Recv.List) > 0 {
		recvField := fn.Recv.List[0]
		if len(recvField.Names) > 0 {
			recvObj := c.pkg.TypesInfo.ObjectOf(recvField.Names[0])
			if recvObj != nil && obj == recvObj {
				return true
			}
		}
	}

	// Def-use analysis for local variables
	return c.isProvenNonNilLocalVar(obj, fn)
}

// isProvenNonNilLocalVar checks if a local variable is always non-nil by
// examining all assignment sites.
func (c *Converter) isProvenNonNilLocalVar(obj types.Object, fn *ast.FuncDecl) bool {
	hasInit := false
	initNonNil := false
	allReassignNonNil := true

	// Walk the entire body including closures — closures can modify captured variables
	ast.Inspect(fn.Body, func(n ast.Node) bool {
		switch s := n.(type) {
		case *ast.AssignStmt:
			for i, lhs := range s.Lhs {
				lhsIdent, ok := lhs.(*ast.Ident)
				if !ok {
					continue
				}
				lhsObj := c.pkg.TypesInfo.ObjectOf(lhsIdent)
				if lhsObj != obj {
					continue
				}

				if s.Tok == token.DEFINE {
					hasInit = true
					if len(s.Lhs) > 1 && len(s.Rhs) == 1 {
						// Multi-value: x, y := foo() — can't prove individual positions
						initNonNil = false
					} else if i < len(s.Rhs) {
						initNonNil = c.isProvenNonNilExpr(s.Rhs[i], fn)
					}
				} else {
					if len(s.Lhs) > 1 && len(s.Rhs) == 1 {
						allReassignNonNil = false
					} else if i < len(s.Rhs) {
						if !c.isProvenNonNilExpr(s.Rhs[i], fn) {
							allReassignNonNil = false
						}
					} else {
						allReassignNonNil = false
					}
				}
			}

		case *ast.ValueSpec:
			// var x = expr
			for i, name := range s.Names {
				nameObj := c.pkg.TypesInfo.Defs[name]
				if nameObj != obj {
					continue
				}
				hasInit = true
				if i < len(s.Values) {
					initNonNil = c.isProvenNonNilExprSimple(s.Values[i])
				}
				// var x *T with no initializer: initNonNil stays false
			}
		}
		return true
	})

	return hasInit && initNonNil && allReassignNonNil
}

// isProvenNonNilCallResult checks if a function call is known to return non-nil.
func (c *Converter) isProvenNonNilCallResult(call *ast.CallExpr) bool {
	var calleeName string
	var calleeObj types.Object

	switch fn := call.Fun.(type) {
	case *ast.Ident:
		calleeName = fn.Name
		if c.pkg.TypesInfo != nil {
			calleeObj = c.pkg.TypesInfo.Uses[fn]
		}
	case *ast.SelectorExpr:
		calleeName = fn.Sel.Name
		if c.pkg.TypesInfo != nil {
			calleeObj = c.pkg.TypesInfo.Uses[fn.Sel]
		}
	default:
		return false
	}

	// Builtins
	if calleeName == "new" || calleeName == "make" {
		return true
	}

	// Constructor name heuristic
	if looksLikeConstructor(calleeName) {
		return true
	}

	// Config override and same-package transitive analysis
	if calleeObj == nil {
		return false
	}
	fn, ok := calleeObj.(*types.Func)
	if !ok {
		return false
	}

	calleePkg := ""
	qualName := calleeName
	if fn.Pkg() != nil {
		calleePkg = fn.Pkg().Path()
	}

	// For methods, qualify with receiver type name
	if sig, ok := fn.Type().(*types.Signature); ok && sig.Recv() != nil {
		recvType := sig.Recv().Type()
		if ptr, ok := recvType.(*types.Pointer); ok {
			if named, ok := ptr.Elem().(*types.Named); ok {
				qualName = named.Obj().Name() + "." + calleeName
			}
		} else if named, ok := recvType.(*types.Named); ok {
			qualName = named.Obj().Name() + "." + calleeName
		}
	}

	if c.cfg != nil && c.cfg.IsNonNilableReturn(calleePkg, qualName) {
		return true
	}

	// Same-package transitive analysis with cycle detection
	if calleePkg == c.currentPkgPath {
		return c.isProvenNonNilFunc(calleeObj)
	}

	// Cross-package transitive analysis (one level deep)
	if !c.noCrossPkg && calleePkg != "" {
		return c.isProvenNonNilCrossPkgFunc(calleeObj, calleePkg)
	}

	return false
}

// isProvenNonNilFunc recursively checks if a same-package function is proven
// non-nil. Results are cached; cycles are detected and conservatively return false.
func (c *Converter) isProvenNonNilFunc(obj types.Object) bool {
	if c.nonNilCache == nil {
		c.nonNilCache = make(map[token.Pos]nilCacheResult)
	}

	pos := obj.Pos()
	if cached, ok := c.nonNilCache[pos]; ok {
		return cached == nilCacheProven
	}

	// Mark in-progress for cycle detection
	c.nonNilCache[pos] = nilCacheInProgress

	fn := c.findFuncDecl(obj)
	if fn == nil || fn.Body == nil {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}

	sig, ok := obj.Type().(*types.Signature)
	if !ok || !isSinglePointerResult(sig) {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}

	if c.analyzeReturnNilability(fn) {
		c.nonNilCache[pos] = nilCacheProven
		return true
	}

	c.nonNilCache[pos] = nilCacheNotProven
	return false
}

// isProvenNonNilCrossPkgFunc checks if a function in an imported package is
// proven non-nil. Limited to one level deep.
func (c *Converter) isProvenNonNilCrossPkgFunc(obj types.Object, pkgPath string) bool {
	if c.nonNilCache == nil {
		c.nonNilCache = make(map[token.Pos]nilCacheResult)
	}

	pos := obj.Pos()
	if cached, ok := c.nonNilCache[pos]; ok {
		return cached == nilCacheProven
	}

	if c.pkg == nil || c.pkg.Imports == nil {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}
	importedPkg, ok := c.pkg.Imports[pkgPath]
	if !ok || importedPkg == nil || importedPkg.Syntax == nil || importedPkg.TypesInfo == nil {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}

	sig, ok := obj.Type().(*types.Signature)
	if !ok || !isSinglePointerResult(sig) {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}

	if c.crossPkgConverters == nil {
		c.crossPkgConverters = make(map[string]*Converter)
	}
	tempConv, ok := c.crossPkgConverters[pkgPath]
	if !ok {
		tempConv = NewConverter(pkgPath, importedPkg, c.cfg)
		tempConv.noCrossPkg = true
		c.crossPkgConverters[pkgPath] = tempConv
	}

	fn := tempConv.findFuncDecl(obj)
	if fn == nil || fn.Body == nil {
		c.nonNilCache[pos] = nilCacheNotProven
		return false
	}

	if tempConv.analyzeReturnNilability(fn) {
		c.nonNilCache[pos] = nilCacheProven
		return true
	}

	c.nonNilCache[pos] = nilCacheNotProven
	return false
}

// isAssignedNonNilInInit checks if a package-level variable is assigned a
// non-nil value in an init() function.
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

// ncIsInsideNilReceiverGuard checks if a return statement is inside an
// `if recv == nil { ... }` block at the top level of the function body.
func ncIsInsideNilReceiverGuard(body *ast.BlockStmt, ret *ast.ReturnStmt, receiverName string) bool {
	for _, stmt := range body.List {
		ifStmt, ok := stmt.(*ast.IfStmt)
		if !ok {
			continue
		}
		if ncIsNilCheck(ifStmt.Cond, receiverName) && ncContainsReturn(ifStmt.Body, ret) {
			return true
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

func ncIsNilExpr(expr ast.Expr) bool {
	ident, ok := expr.(*ast.Ident)
	return ok && ident.Name == "nil"
}

func ncIsAddressOfLiteral(expr ast.Expr) bool {
	unary, ok := expr.(*ast.UnaryExpr)
	if !ok || unary.Op != token.AND {
		return false
	}
	_, isLit := unary.X.(*ast.CompositeLit)
	return isLit
}

func ncIsNilCheck(cond ast.Expr, name string) bool {
	binExpr, ok := cond.(*ast.BinaryExpr)
	if !ok || binExpr.Op != token.EQL {
		return false
	}
	return (ncIsIdentNamed(binExpr.X, name) && ncIsNilExpr(binExpr.Y)) ||
		(ncIsNilExpr(binExpr.X) && ncIsIdentNamed(binExpr.Y, name))
}

func ncIsIdentNamed(expr ast.Expr, name string) bool {
	ident, ok := expr.(*ast.Ident)
	return ok && ident.Name == name
}

func ncContainsReturn(block *ast.BlockStmt, target *ast.ReturnStmt) bool {
	found := false
	ast.Inspect(block, func(n ast.Node) bool {
		if n == target {
			found = true
			return false
		}
		return true
	})
	return found
}
