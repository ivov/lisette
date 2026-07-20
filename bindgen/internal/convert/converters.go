package convert

import (
	"fmt"
	"go/ast"
	"go/constant"
	"go/token"
	"go/types"
	"maps"
	"slices"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

var reservedKeywords = map[string]bool{
	"fn": true, "let": true, "if": true, "else": true,
	"match": true, "enum": true, "struct": true, "type": true,
	"interface": true, "impl": true, "const": true, "var": true,
	"return": true, "defer": true, "import": true, "mut": true,
	"pub": true, "for": true, "in": true, "while": true,
	"loop": true, "break": true, "continue": true, "select": true,
	"task": true, "try": true, "recover": true, "self": true,
	"as": true,
}

func sanitizeParamName(name string) string {
	if reservedKeywords[name] {
		return name + "_"
	}

	if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
		return strings.ToLower(name[:1]) + name[1:]
	}

	return name
}

func isReferenceType(typeStr string) bool {
	return isSliceType(typeStr) || isMapType(typeStr)
}

// liftReflectionDecodeParams returns (specs, nil) when not whitelisted or no
// `interface{}` params are liftable; the index map encodes per-param Ref<T>
// rewrites for the caller to apply during the param loop.
func (c *Converter) liftReflectionDecodeParams(
	sig *types.Signature,
	qualifiedName string,
	specs TypeParamSpecs,
) (TypeParamSpecs, map[int]string) {
	if !c.cfg.IsReflectionDecode(c.currentPkgPath, qualifiedName) {
		return specs, nil
	}
	used := make(map[string]bool, len(specs))
	for _, s := range specs {
		used[s.Name] = true
	}
	var overrides map[int]string
	params := sig.Params()
	for i := 0; i < params.Len(); i++ {
		if sig.Variadic() && i == params.Len()-1 {
			continue
		}
		t := params.At(i).Type()
		for {
			alias, ok := t.(*types.Alias)
			if !ok {
				break
			}
			t = alias.Rhs()
		}
		iface, ok := t.(*types.Interface)
		if !ok || !iface.Empty() || isErrorInterface(iface) {
			continue
		}
		name := freshTypeParamName(used)
		used[name] = true
		specs = append(specs, TypeParamSpec{Name: name})
		if overrides == nil {
			overrides = make(map[int]string)
		}
		overrides[i] = refOf(name)
	}
	return specs, overrides
}

func freshTypeParamName(used map[string]bool) string {
	if !used["T"] {
		return "T"
	}
	for n := 2; ; n++ {
		candidate := fmt.Sprintf("T%d", n)
		if !used[candidate] {
			return candidate
		}
	}
}

func (c *Converter) convertFunction(result *ConvertResult, symbolExport extract.SymbolExport) {
	signature, ok := symbolExport.GoType.(*types.Signature)
	if !ok {
		result.SkipReason = &SkipReason{Code: "invalid-signature", Message: "not a function signature"}
		return
	}

	// Scan first: param/return processing must observe `S ~[]E` substitutions.
	typeParams, substitutions, recipe, skip := collectTypeParams(signature.TypeParams(), false, c)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.TypeParams = typeParams
	if len(substitutions) > 0 {
		result.CollapsedTypeParamRecipe = strings.Join(recipe, ", ")
	}

	prevSubs := c.typeParamSubstitutions
	c.typeParamSubstitutions = substitutions
	defer func() { c.typeParamSubstitutions = prevSubs }()

	liftedSpecs, paramOverrides := c.liftReflectionDecodeParams(signature, result.Name, result.TypeParams)
	result.TypeParams = liftedSpecs

	params, skip := c.convertParams(signature, result.Name, result.Name, paramOverrides, true)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.Params = params

	returnType := c.applyReturnType(result, signature, result.Name)

	c.resolveNilability(result, signature, returnType, nilabilityDecision{
		obj:               symbolExport.Obj,
		lookups:           []configKey{{c.currentPkgPath, result.Name}},
		nameIsConstructor: looksLikeConstructor(result.Name),
		heuristicNonNil: func(isSinglePointerReturn bool) bool {
			if looksLikeConstructor(result.Name) {
				return true
			}
			if isSinglePointerReturn && isPointerBoxingFunction(signature) {
				return true
			}
			if isSinglePointerReturn && isIteratorReturnType(signature) {
				return true
			}
			if isSinglePointerReturn && c.isManyToOneFactory(signature) {
				return true
			}
			if isSinglePointerReturn && c.hasMatchingSelfReturningMethod(result.Name, signature) {
				return true
			}
			return false
		},
	})
}

// applySentinelInt rewrites a bare `int` return into `Option<int>` when
// the config declares a sentinel; emit then writes the matching flag.
func (c *Converter) applySentinelInt(result *ConvertResult, qualifiedName string) {
	if c.cfg == nil || result.ReturnType != "int" {
		return
	}
	value, ok := c.cfg.SentinelInt(c.currentPkgPath, qualifiedName)
	if !ok {
		return
	}
	result.ReturnType = "Option<int>"
	result.SentinelInt = &value
}

func (c *Converter) convertParams(sig *types.Signature, lookupName, methodName string, paramOverrides map[int]string, directEligible bool) ([]FunctionParameter, *SkipReason) {
	mutParams := c.cfg.MutatingParams(c.currentPkgPath, lookupName)
	nilableParams := c.cfg.NilableParams(c.currentPkgPath, lookupName)

	params := sig.Params()
	usedNames := collectNamedParams(params)
	var out []FunctionParameter
	for i := 0; i < params.Len(); i++ {
		param := params.At(i)
		name := param.Name()
		if name == "" {
			isVariadic := sig.Variadic() && i == params.Len()-1
			name = deriveParamName(param.Type(), i, isVariadic, usedNames)
		} else {
			name = sanitizeParamName(name)
		}

		var paramType TypeResult
		if named, ok := c.directHandleIfEligible(param.Type(), directEligible); ok {
			paramType = TypeResult{LisetteType: named.Obj().Name()}
		} else {
			paramType = convertParamType(param.Type(), name, nilableParams, c)
		}
		if paramType.SkipReason != nil {
			return nil, paramType.SkipReason
		}

		typeStr := paramType.LisetteType
		if sig.Variadic() && i == params.Len()-1 {
			typeStr = sliceToVarArgs(typeStr)
		}
		if override, ok := paramOverrides[i]; ok {
			typeStr = override
		}

		out = append(out, FunctionParameter{
			Name:    name,
			Type:    typeStr,
			Mutable: isMutableParam(mutParams, name, typeStr, methodName),
		})
	}
	return out, nil
}

func (c *Converter) applyReturnType(result *ConvertResult, sig *types.Signature, lookupName string) TypeResult {
	returnType := ReturnsToLisette(sig, c, lookupName)
	if returnType.LisetteType != "" {
		result.ReturnType = returnType.LisetteType
	} else if returnType.SkipReason != nil {
		result.ReturnType = "Unknown"
	}
	if returnType.SkipReason != nil {
		result.SkipNote = returnType.SkipReason
	}
	result.CommaOk = returnType.CommaOk
	c.applySentinelInt(result, lookupName)
	return returnType
}

type configKey struct {
	pkgPath string
	name    string
}

type nilabilityDecision struct {
	obj types.Object
	// lookups: config keys consulted in order, first match wins.
	lookups []configKey
	// heuristicNonNil: kind-specific rules, used only when there is no body to analyze.
	heuristicNonNil func(isSinglePointerReturn bool) bool
	// nameIsConstructor: the only heuristic trusted over an inconclusive body, since
	// weaker shape heuristics misfire on real nil-returning delegators like big.Int.Exp.
	nameIsConstructor bool
}

// resolveNilability wraps the return in Option<> unless body proof, a name
// heuristic, or config pins it non-nilable (in that precedence).
func (c *Converter) resolveNilability(result *ConvertResult, sig *types.Signature, returnType TypeResult, d nilabilityDecision) {
	isSingleNilableReturn := isSingleNilableResult(sig)
	if isSingleNilableReturn && returnType.IsDirectError {
		isSingleNilableReturn = false
	}
	isSinglePointerReturn := isSingleNilableReturn && isSinglePointerResult(sig)

	forceNonNilable := false
	if isSingleNilableReturn {
		fn := c.findFuncDecl(d.obj)
		if fn == nil {
			forceNonNilable = d.heuristicNonNil(isSinglePointerReturn)
		} else {
			switch c.analyzeReturnNilability(fn) {
			case returnNilabilityProvenNonNil:
				forceNonNilable = true
			case returnNilabilityInconclusive:
				forceNonNilable = d.nameIsConstructor
			}
		}
	}
	for _, k := range d.lookups {
		if forceNonNilable {
			break
		}
		forceNonNilable = c.cfg.IsNonNilableReturn(k.pkgPath, k.name)
	}

	forceNilable := false
	for _, k := range d.lookups {
		if forceNilable {
			break
		}
		forceNilable = c.cfg.ShouldWrapNilableReturn(k.pkgPath, k.name)
	}

	if (isSingleNilableReturn && !forceNonNilable) || (forceNilable && !returnType.NilableReturnApplied) {
		result.ReturnType = optionOf(result.ReturnType)
	}
}

func (c *Converter) convertMethod(result *ConvertResult, symbolExport extract.SymbolExport) {
	signature, ok := symbolExport.GoType.(*types.Signature)
	if !ok {
		result.SkipReason = &SkipReason{Code: "invalid-signature", Message: "not a method signature"}
		return
	}

	if symbolExport.ReceiverVariable != nil {
		if symbolExport.IsPromoted {
			typeName := symbolExport.BaseType.Obj().Name()
			typeParams := extractReceiverTypeParams(symbolExport.BaseType, c)
			_, isPointerReceiver := symbolExport.ReceiverVariable.Type().(*types.Pointer)

			recvLisetteType := typeName
			if isPointerReceiver {
				recvLisetteType = refOf(typeName)
			}

			result.Receiver = &Receiver{
				Name:         symbolExport.ReceiverVariable.Name(),
				Type:         recvLisetteType,
				IsPointer:    isPointerReceiver,
				BaseTypeName: typeName,
				TypeParams:   typeParams,
			}
		} else {
			isPointerReceiver := false
			typeName := ""
			var typeParams TypeParamSpecs
			if pointer, ok := symbolExport.ReceiverVariable.Type().(*types.Pointer); ok {
				isPointerReceiver = true
				if named, ok := pointer.Elem().(*types.Named); ok {
					typeName = named.Obj().Name()
					typeParams = extractReceiverTypeParams(named, c)
				}
			} else if named, ok := symbolExport.ReceiverVariable.Type().(*types.Named); ok {
				typeName = named.Obj().Name()
				typeParams = extractReceiverTypeParams(named, c)
			}

			// An unexported same-package struct receiver renders by its bare name;
			// ToLisette has no name for it, so bypass it.
			var recvLisetteType string
			if _, ok := unexportedSamePkgStruct(symbolExport.ReceiverVariable.Type(), c); ok {
				recvLisetteType = typeName
				if isPointerReceiver {
					recvLisetteType = refOf(typeName)
				}
			} else {
				recvType := ToLisette(symbolExport.ReceiverVariable.Type(), c)
				if recvType.SkipReason != nil {
					result.SkipReason = recvType.SkipReason
					return
				}
				recvLisetteType = recvType.LisetteType
			}

			result.Receiver = &Receiver{
				Name:         symbolExport.ReceiverVariable.Name(),
				Type:         recvLisetteType,
				IsPointer:    isPointerReceiver,
				BaseTypeName: typeName,
				TypeParams:   typeParams,
			}
		}
	}

	// A seal is recorded by identity and receiver only; its params and return are
	// unused and may be unrepresentable.
	if symbolExport.Unexported {
		pkgPath := ""
		if symbolExport.Obj != nil && symbolExport.Obj.Pkg() != nil {
			pkgPath = symbolExport.Obj.Pkg().Path()
		}
		result.SealId = sealIdentity(pkgPath, result.Name, signature)
		return
	}

	qualifiedName := result.Name
	if result.Receiver != nil && result.Receiver.BaseTypeName != "" {
		qualifiedName = result.Receiver.BaseTypeName + "." + result.Name
	}

	methodSpecs, _, _, skip := collectTypeParams(signature.TypeParams(), false, c)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	liftedSpecs, paramOverrides := c.liftReflectionDecodeParams(signature, qualifiedName, methodSpecs)

	params, skip := c.convertParams(signature, qualifiedName, result.Name, paramOverrides, true)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.Params = params

	returnType := c.applyReturnType(result, signature, qualifiedName)

	lookups := []configKey{{c.currentPkgPath, qualifiedName}}
	if symbolExport.IsPromoted && symbolExport.OriginalTypeName != "" {
		lookups = append(lookups, configKey{symbolExport.OriginalPkgPath, symbolExport.OriginalTypeName + "." + result.Name})
	}
	c.resolveNilability(result, signature, returnType, nilabilityDecision{
		obj:               symbolExport.Obj,
		lookups:           lookups,
		nameIsConstructor: looksLikeConstructor(result.Name),
		heuristicNonNil: func(isSinglePointerReturn bool) bool {
			if looksLikeConstructor(result.Name) {
				return true
			}
			if isSinglePointerReturn && result.Receiver != nil && !looksLikeNavigationMethod(result.Name) {
				if isSelfReturning(signature, result.Receiver.BaseTypeName) ||
					c.isUniformPointerReturnType(result.Receiver.BaseTypeName) ||
					c.isMajorityPointerReturnType(result.Receiver.BaseTypeName) {
					return true
				}
			}
			if isSinglePointerReturn && symbolExport.IsPromoted && symbolExport.OriginalTypeName != "" {
				if isSelfReturning(signature, symbolExport.OriginalTypeName) {
					return true
				}
			}
			if isSinglePointerReturn && isIteratorReturnType(signature) {
				return true
			}
			return false
		},
	})

	if symbolExport.BaseType != nil {
		_, substitutions, _, skip := collectTypeParams(symbolExport.BaseType.TypeParams(), false, c)
		if skip != nil {
			result.SkipReason = skip
			return
		}
		if len(substitutions) > 0 {
			result.SkipReason = &SkipReason{
				Code:    "collapsed-type-param",
				Message: "receiver type has a shape-collapsed type parameter",
			}
			return
		}
	}

	result.TypeParams = liftedSpecs

	if isFluentBuilderCandidate(result, symbolExport, signature) {
		if fn := c.findFuncDecl(symbolExport.Obj); fn != nil && isFluentMethod(fn, ncGetReceiverName(fn)) {
			if c.cfg == nil || !c.cfg.ShouldDenyUnusedValue(c.currentPkgPath, qualifiedName) {
				result.BuilderMethod = true
			}
		}
	}
}

// isFluentBuilderCandidate gates AST inspection. Clone/Copy return new values despite the fluent shape.
func isFluentBuilderCandidate(result *ConvertResult, exp extract.SymbolExport, sig *types.Signature) bool {
	if result.Receiver == nil || !result.Receiver.IsPointer {
		return false
	}
	if result.Name == "Clone" || result.Name == "Copy" {
		return false
	}
	return returnIsReceiverShaped(sig)
}

// leftmostIdent walks `recv.A(...).B(...)` chains so the receiver can be detected at the head.
func leftmostIdent(expr ast.Expr) *ast.Ident {
	switch e := expr.(type) {
	case *ast.Ident:
		return e
	case *ast.SelectorExpr:
		return leftmostIdent(e.X)
	case *ast.CallExpr:
		return leftmostIdent(e.Fun)
	}
	return nil
}

// returnIsReceiverShaped filters out delegation that returns unrelated types (e.g. `*Alpha.At -> color.Color`) and Result/Option-wrapped returns where unused_value cannot fire.
func returnIsReceiverShaped(sig *types.Signature) bool {
	recv := sig.Recv()
	if recv == nil {
		return false
	}
	results := sig.Results()
	if results.Len() != 1 {
		return false
	}
	recvPtr, ok := recv.Type().(*types.Pointer)
	if !ok {
		return false
	}
	recvNamed, ok := recvPtr.Elem().(*types.Named)
	if !ok {
		return false
	}

	if retNamed := singlePointerReturnNamed(sig); retNamed != nil && retNamed == recvNamed {
		return true
	}
	retNamed, ok := results.At(0).Type().(*types.Named)
	if !ok {
		return false
	}
	if retNamed == recvNamed {
		return true
	}
	if iface, ok := retNamed.Underlying().(*types.Interface); ok && !iface.Empty() {
		return types.Implements(recvPtr, iface)
	}
	return false
}

// FinalizeInterfaceBuilders carries the BuilderMethod flag from concrete methods to matching interface methods, when a concrete implementer is itself marked.
func (c *Converter) FinalizeInterfaceBuilders(results []ConvertResult) {
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}

	concreteBuilders := make(map[string]map[string]bool)
	for _, r := range results {
		if r.Kind != extract.ExportMethod || r.Receiver == nil || !r.BuilderMethod {
			continue
		}
		methods, ok := concreteBuilders[r.Receiver.BaseTypeName]
		if !ok {
			methods = make(map[string]bool)
			concreteBuilders[r.Receiver.BaseTypeName] = methods
		}
		methods[r.Name] = true
	}
	if len(concreteBuilders) == 0 {
		return
	}

	scope := c.pkg.Types.Scope()
	for i := range results {
		result := &results[i]
		if result.Kind != extract.ExportType || !result.IsInterface || len(result.InterfaceMethods) == 0 {
			continue
		}
		ifaceObj := scope.Lookup(result.Name)
		if ifaceObj == nil {
			continue
		}
		ifaceNamed, ok := ifaceObj.Type().(*types.Named)
		if !ok {
			continue
		}
		iface, ok := ifaceNamed.Underlying().(*types.Interface)
		if !ok {
			continue
		}
		for mi := range result.InterfaceMethods {
			methodName := result.InterfaceMethods[mi].Name
			for concreteName, builderMethods := range concreteBuilders {
				if !builderMethods[methodName] {
					continue
				}
				concreteObj := scope.Lookup(concreteName)
				if concreteObj == nil {
					continue
				}
				concreteNamed, ok := concreteObj.Type().(*types.Named)
				if !ok {
					continue
				}
				if types.Implements(types.NewPointer(concreteNamed), iface) {
					result.InterfaceMethods[mi].BuilderMethod = true
					break
				}
			}
		}
	}
}

// isFluentMethod excludes trivial `return self` getters — real fluent setters either do work before returning or delegate via a method call on the receiver.
func isFluentMethod(fn *ast.FuncDecl, recvName string) bool {
	if fn == nil || fn.Body == nil || recvName == "" {
		return false
	}

	hasReturn := false
	allMatchRecv := true
	anyCallOnRecv := false
	ast.Inspect(fn.Body, func(n ast.Node) bool {
		if _, ok := n.(*ast.FuncLit); ok {
			return false
		}
		ret, ok := n.(*ast.ReturnStmt)
		if !ok {
			return true
		}
		if len(ret.Results) != 1 {
			allMatchRecv = false
			return true
		}
		hasReturn = true
		switch r := ret.Results[0].(type) {
		case *ast.Ident:
			if r.Name != recvName {
				allMatchRecv = false
			}
		case *ast.CallExpr:
			if id := leftmostIdent(r); id != nil && id.Name == recvName {
				anyCallOnRecv = true
				return true
			}
			allMatchRecv = false
		default:
			allMatchRecv = false
		}
		return true
	})

	if !hasReturn || !allMatchRecv {
		return false
	}
	return len(fn.Body.List) > 1 || anyCallOnRecv
}

func (c *Converter) convertType(result *ConvertResult, exp extract.SymbolExport) {
	if alias, ok := exp.GoType.(*types.Alias); ok {
		if isGenericAlias(alias) {
			result.SkipReason = &SkipReason{
				Code:    "generic-alias",
				Message: "generic type aliases are not yet representable",
			}
			return
		}
		rhs := alias.Rhs()
		t := ToLisette(rhs, c)
		if t.SkipReason != nil {
			if newtype, ok := c.salvageInternalAlias(rhs, t.SkipReason); ok {
				result.LisetteType = newtype
				return
			}
			result.SkipReason = withOpaqueType(t.SkipReason)
			return
		}
		result.LisetteType = t.LisetteType
		result.IsTypeAlias = true
		return
	}

	if basic, ok := exp.GoType.(*types.Basic); ok && basic.Kind() == types.UnsafePointer {
		result.SkipReason = &SkipReason{
			Code:           "unrepresentable-builtin",
			Message:        "Lisette has no untyped pointer type",
			EmitOpaqueType: true,
		}
		return
	}

	named, ok := exp.GoType.(*types.Named)
	if !ok {
		result.SkipReason = &SkipReason{Code: "not-named-type", Message: "expected named type"}
		return
	}

	typeParams, substitutions, _, skip := collectTypeParams(named.TypeParams(), true, c)
	if skip != nil {
		result.TypeParams = bareTypeParamSpecs(named.TypeParams())
		result.SkipReason = skip
		return
	}
	// Shape collapse (`S ~[]E`) is only supported for functions, not types.
	if len(substitutions) > 0 {
		result.TypeParams = bareTypeParamSpecs(named.TypeParams())
		result.SkipReason = &SkipReason{
			Code:           "collapsed-type-param",
			Message:        "generic type with a shape-collapsed type parameter is not representable",
			EmitOpaqueType: true,
		}
		return
	}
	result.TypeParams = typeParams
	result.UnexportedType = exp.Unexported

	underlying := named.Underlying()

	switch u := underlying.(type) {
	case *types.Struct:
		result.Fields = convertStructFields(u, c)
		result.HasHiddenEmbed = structHasHiddenEmbed(u, result.Fields)

	case *types.Interface:
		if isErrorInterface(u) {
			result.LisetteType = "error"
		} else if u.Empty() {
			result.LisetteType = "Unknown"
			result.IsTypeAlias = true
		} else if methods, ok := c.extractInterfaceMethods(u, result.Name); ok {
			result.IsInterface = true
			result.InterfaceMethods = methods
		}

	case *types.Basic:
		result.LisetteType = basicToLisette(u)

	default:
		t := ToLisette(underlying, c)
		if t.SkipReason != nil {
			result.SkipReason = withOpaqueType(t.SkipReason)
			return
		}
		result.LisetteType = t.LisetteType
	}
}

// withOpaqueType clones reason with EmitOpaqueType set, so a skipped top-level
// type still leaves a `pub type X` placeholder for downstream references.
func withOpaqueType(reason *SkipReason) *SkipReason {
	copied := *reason
	copied.EmitOpaqueType = true
	return &copied
}

func (c *Converter) convertConstant(result *ConvertResult, exp extract.SymbolExport) {
	constObj, ok := exp.Obj.(*types.Const)
	if !ok {
		result.SkipReason = &SkipReason{Code: "invalid-const", Message: "not a constant"}
		return
	}

	val := constObj.Val()
	if val != nil {
		if original := c.getOriginalLiteral(constObj); original != "" {
			result.ConstValue = original
		} else {
			result.ConstValue = formatConstantValue(val)
		}
	}

	actualType := exp.GoType
	if isBasicType(exp.GoType) {
		if rhsType := c.inferRhsType(constObj); rhsType != nil && !isBasicType(rhsType) {
			actualType = rhsType
		}
	}

	t := ToLisette(actualType, c)
	if t.SkipReason != nil {
		// Const typed by an internal package stays referenceable as an opaque
		// value; the rendered Lisette code emits the qualified name verbatim,
		// and Go preserves the original const's type at the call site. Mirrors
		// the function-return fallback in convertFunction.
		if t.SkipReason.Code == "internal-package-ref" {
			result.LisetteType = "Unknown"
			result.ConstValue = ""
			result.SkipNote = t.SkipReason
			return
		}
		result.SkipReason = t.SkipReason
		return
	}
	result.LisetteType = t.LisetteType
}

func (c *Converter) convertVariable(result *ConvertResult, exp extract.SymbolExport) {
	if named, ok := c.directHandle(exp.GoType); ok {
		result.LisetteType = named.Obj().Name()
		return
	}

	t := ToLisette(exp.GoType, c)
	if t.SkipReason != nil && t.SkipReason.Code != "internal-package-ref" {
		result.SkipReason = t.SkipReason
		return
	}
	if t.SkipReason != nil {
		result.LisetteType = "Unknown"
		result.SkipNote = t.SkipReason
	} else {
		result.LisetteType = t.LisetteType
	}

	isNilable := isNilableGoType(exp.GoType)
	forceNonNilable := c.cfg != nil && (c.cfg.IsNonNilableVar(c.currentPkgPath, result.Name) || c.cfg.IsNonNilableReturn(c.currentPkgPath, result.Name))
	if !forceNonNilable && isNilable {
		forceNonNilable = c.isProvenNonNilVar(exp.Obj)
	}
	if isNilable && !forceNonNilable {
		result.LisetteType = optionOf(result.LisetteType)
	}
}

func convertStructFields(s *types.Struct, c *Converter) []StructField {
	var fields []StructField
	for field := range s.Fields() {
		if !field.Exported() {
			if field.Embedded() && c.EmbedIsFaithful(field) {
				fields = append(fields, embeddedStructField(field, c))
			}
			continue
		}
		if field.Embedded() {
			fields = append(fields, embeddedStructField(field, c))
			continue
		}
		fieldType := ToLisetteNilable(field.Type(), c)
		if fieldType.SkipReason != nil {
			fields = append(fields, StructField{
				Name:       field.Name(),
				SkipReason: fieldType.SkipReason,
			})
			continue
		}
		fields = append(fields, StructField{
			Name: field.Name(),
			Type: fieldType.LisetteType,
		})
	}
	return fields
}

// EmbedIsFaithful reports whether bindgen emits field as an `embed`.
func (c *Converter) EmbedIsFaithful(field *types.Var) bool {
	if !field.Exported() {
		named, ok := unexportedSamePkgStruct(field.Type(), c)
		if !ok {
			return false
		}
		st := named.Underlying().(*types.Struct)
		for embed := range st.Fields() {
			if embed.Embedded() && !c.EmbedIsFaithful(embed) {
				return false
			}
		}
		return true
	}
	// Probe runs before the owner's Convert checkpoint; snapshot/restore so its synth structs and imports do not leak.
	synthMark := c.synthMark()
	savedPkgs := make(ExternalPkgs, len(c.externalPkgs))
	maps.Copy(savedPkgs, c.externalPkgs)
	faithful := embeddedStructField(field, c).IsEmbedded
	c.rollbackSynth(synthMark)
	c.externalPkgs = savedPkgs
	return faithful
}

// unexportedSamePkgStruct returns the named type behind t (peeling one pointer)
// when it is an unexported struct declared in the package being generated, which
// is emitted as an opaque `#[go(unexported)]` type the outer struct can embed.
func unexportedSamePkgStruct(t types.Type, c *Converter) (*types.Named, bool) {
	if ptr, ok := t.(*types.Pointer); ok {
		t = ptr.Elem()
	}
	named, ok := t.(*types.Named)
	if !ok {
		return nil, false
	}
	obj := named.Obj()
	if obj.Exported() || obj.Pkg() == nil || obj.Pkg().Path() != c.currentPkgPath {
		return nil, false
	}
	if _, ok := named.Underlying().(*types.Struct); !ok {
		return nil, false
	}
	if named.TypeParams().Len() > 0 || named.TypeArgs().Len() > 0 {
		return nil, false
	}
	return named, true
}

func embeddedStructField(field *types.Var, c *Converter) StructField {
	target := field.Type()
	isPointer := false
	if ptr, ok := target.(*types.Pointer); ok {
		target = ptr.Elem()
		isPointer = true
	}
	if named, ok := unexportedSamePkgStruct(target, c); ok {
		ref := named.Obj().Name()
		if isPointer {
			ref = refOf(ref)
		}
		return StructField{Name: field.Name(), Type: ref, IsEmbedded: true}
	}
	// Generic type aliases are not yet representable, so do not embed one.
	if isGenericAlias(target) {
		return StructField{Name: field.Name(), SkipReason: &SkipReason{
			Code:    "generic-alias-embed",
			Message: "generic type alias embedding is not yet representable",
		}}
	}
	elem := ToLisetteNilable(target, c)
	if elem.SkipReason != nil {
		return StructField{Name: field.Name(), SkipReason: elem.SkipReason}
	}
	_, isStruct := target.Underlying().(*types.Struct)
	bare := elem.LisetteType
	// Keep the alias spelling as the embed target (h.Alias, not its RHS h.Base).
	if isStruct {
		if alias, ok := target.(*types.Alias); ok {
			bare = aliasEmbedTarget(alias, c)
		}
	}
	ref := bare
	if isPointer {
		ref = refOf(ref)
	}
	if !isStruct {
		return StructField{Name: field.Name(), Type: ref}
	}
	return StructField{Name: field.Name(), Type: ref, IsEmbedded: true}
}

// aliasEmbedTarget returns the alias's own name (qualified when external), not its RHS.
func aliasEmbedTarget(alias *types.Alias, c *Converter) string {
	obj := alias.Obj()
	if pkg := obj.Pkg(); pkg != nil && pkg.Path() != c.currentPkgPath {
		c.trackExternalPkg(pkg.Path(), pkg.Name())
		return fmt.Sprintf("%s.%s", PkgRef(pkg.Path()), obj.Name())
	}
	return obj.Name()
}

// isGenericAlias reports whether t is a generic type alias (or an instantiation of one).
func isGenericAlias(t types.Type) bool {
	alias, ok := t.(*types.Alias)
	if !ok {
		return false
	}
	return alias.TypeParams().Len() > 0 || alias.TypeArgs().Len() > 0
}

// structHasHiddenEmbed reports whether s has an embed bindgen did not emit, so it looks flat but is not.
func structHasHiddenEmbed(s *types.Struct, emitted []StructField) bool {
	goEmbeds := 0
	for field := range s.Fields() {
		if field.Embedded() {
			goEmbeds++
		}
	}
	faithful := 0
	for _, f := range emitted {
		if f.IsEmbedded {
			faithful++
		}
	}
	return goEmbeds > faithful
}

func (c *Converter) getOriginalLiteral(constObj *types.Const) string {
	if c.pkg == nil {
		return ""
	}

	pos := constObj.Pos()
	if !pos.IsValid() {
		return ""
	}

	for _, file := range c.pkg.Syntax {
		if file == nil {
			continue
		}

		tokenFile := c.pkg.Fset.File(file.Pos())
		if tokenFile == nil || int(pos) < tokenFile.Base() || int(pos) >= tokenFile.Base()+tokenFile.Size() {
			continue
		}

		var literal string
		ast.Inspect(file, func(n ast.Node) bool {
			vs, ok := n.(*ast.ValueSpec)
			if !ok {
				return true
			}

			for i, name := range vs.Names {
				if name.Pos() == pos && i < len(vs.Values) {
					switch v := vs.Values[i].(type) {
					case *ast.BasicLit:
						if v.Kind == token.INT {
							literal = v.Value
						}
					case *ast.UnaryExpr:
						// Handle negative numbers: -0x8000
						if v.Op == token.SUB {
							if lit, ok := v.X.(*ast.BasicLit); ok && lit.Kind == token.INT {
								literal = "-" + lit.Value
							}
						}
					}
				}
			}
			return literal == ""
		})

		if literal != "" {
			return normalizeLegacyOctal(literal)
		}
	}

	return ""
}

func normalizeLegacyOctal(literal string) string {
	sign := ""
	digits := literal
	if strings.HasPrefix(digits, "-") {
		sign = "-"
		digits = digits[1:]
	}
	if len(digits) < 2 || digits[0] != '0' {
		return literal
	}
	switch digits[1] {
	case 'x', 'X', 'o', 'O', 'b', 'B':
		return literal
	}
	return sign + "0o" + digits[1:]
}

func isBasicType(t types.Type) bool {
	_, ok := t.(*types.Basic)
	return ok
}

// inferRhsType tries to infer the actual type of a constant by examining its RHS
// in the AST. For example, for `const ModePerm = fs.ModePerm`, it returns
// the type of fs.ModePerm (which is fs.FileMode), not the inferred int type.
func (c *Converter) inferRhsType(constObj *types.Const) types.Type {
	if c.pkg == nil || c.pkg.TypesInfo == nil {
		return nil
	}

	pos := constObj.Pos()
	if !pos.IsValid() {
		return nil
	}

	var foundType types.Type
	for _, file := range c.pkg.Syntax {
		if file == nil {
			continue
		}

		tokenFile := c.pkg.Fset.File(file.Pos())
		if tokenFile == nil || int(pos) < tokenFile.Base() || int(pos) >= tokenFile.Base()+tokenFile.Size() {
			continue
		}

		ast.Inspect(file, func(n ast.Node) bool {
			vs, ok := n.(*ast.ValueSpec)
			if !ok {
				return true
			}

			for i, name := range vs.Names {
				if name.Pos() == pos && i < len(vs.Values) {
					rhsExpr := vs.Values[i]
					if tv, ok := c.pkg.TypesInfo.Types[rhsExpr]; ok {
						foundType = tv.Type
					}
				}
			}
			return foundType == nil
		})

		if foundType != nil {
			return foundType
		}
	}

	return nil
}

func isSinglePointerResult(sig *types.Signature) bool {
	results := sig.Results()
	if results.Len() != 1 {
		return false
	}
	_, ok := results.At(0).Type().Underlying().(*types.Pointer)
	return ok
}

// isSingleNilableResult reports whether sig has exactly one Go-nilable result.
func isSingleNilableResult(sig *types.Signature) bool {
	results := sig.Results()
	return results.Len() == 1 && isNilableGoType(results.At(0).Type())
}

func sliceToVarArgs(typeStr string) string {
	if elem, ok := unwrapSlice(typeStr); ok {
		return varArgsOf(elem)
	}
	return typeStr
}

func formatConstantValue(val constant.Value) string {
	switch val.Kind() {
	case constant.Float:
		// ExactString() might produce fractions like "18/5" - use %g for valid literals
		f64, _ := constant.Float64Val(val)
		return fmt.Sprintf("%g", f64)

	case constant.Complex:
		realPart := constant.Real(val)
		imagPart := constant.Imag(val)
		realF64, _ := constant.Float64Val(realPart)
		imagF64, _ := constant.Float64Val(imagPart)

		if realF64 == 0 {
			return fmt.Sprintf("%gi", imagF64)
		}
		if imagF64 == 0 {
			return fmt.Sprintf("%g", realF64)
		}
		if imagF64 < 0 {
			return fmt.Sprintf("%g - %gi", realF64, -imagF64)
		}
		return fmt.Sprintf("%g + %gi", realF64, imagF64)

	default:
		return val.ExactString()
	}
}

// `S ~[]E`, `M ~map[K]V`, and `A ~[N]E` shapes go into substitutions (caller
// rewrites `S` to `Slice<E>`, `M` to `Map<K, V>`, or `A` to `Array<E, N>`)
// rather than into specs. Recognized bounds register their imports on conv.
// `recipe` is Go's full type-parameter list in declaration order, each entry as
// a Lisette type (collapsed entries as their shape, kept entries as the bare
// name), so emit can rebuild Go's type arguments when inference cannot.
func collectTypeParams(
	typeParams *types.TypeParamList,
	emitOpaque bool,
	conv *Converter,
) (specs TypeParamSpecs, substitutions map[string]string, recipe []string, skip *SkipReason) {
	if typeParams == nil {
		return nil, nil, nil, nil
	}

	for tp := range typeParams.TypeParams() {
		if shape, ok := collapsedShape(tp.Constraint()); ok {
			if substitutions == nil {
				substitutions = make(map[string]string)
			}
			substitutions[tp.Obj().Name()] = shape
		}
	}

	if len(substitutions) > 0 && conv != nil {
		prev := conv.typeParamSubstitutions
		conv.typeParamSubstitutions = substitutions
		defer func() { conv.typeParamSubstitutions = prev }()
	}

	for tp := range typeParams.TypeParams() {
		name := tp.Obj().Name()
		constraint := tp.Constraint()

		if shape, ok := substitutions[name]; ok {
			recipe = append(recipe, shape)
			continue
		}

		if isAnyConstraint(constraint) {
			specs = append(specs, TypeParamSpec{Name: name})
			recipe = append(recipe, name)
			continue
		}

		if boundExpr, ok := recognizeBound(constraint, conv); ok {
			specs = append(specs, TypeParamSpec{Name: name, Bound: boundExpr})
			recipe = append(recipe, name)
			continue
		}

		iface, _ := constraint.Underlying().(*types.Interface)
		return nil, nil, nil, &SkipReason{
			Code:           "constraint:" + describeConstraint(iface),
			Message:        fmt.Sprintf("type constraint %s cannot be represented", name),
			EmitOpaqueType: emitOpaque,
		}
	}
	return specs, substitutions, recipe, nil
}

func extractReceiverTypeParams(named *types.Named, conv *Converter) TypeParamSpecs {
	origin := named.Origin()
	typeParams := origin.TypeParams()
	if typeParams == nil || typeParams.Len() == 0 {
		return nil
	}

	specs, substitutions, _, skip := collectTypeParams(typeParams, false, conv)
	if skip != nil || len(substitutions) > 0 {
		// Base type emits the skip; impl block falls back to bare names.
		return bareTypeParamSpecs(typeParams)
	}
	return specs
}

func isAnyConstraint(constraint types.Type) bool {
	if constraint == nil {
		return true
	}
	iface, ok := constraint.Underlying().(*types.Interface)
	if !ok {
		return false
	}
	return iface.Empty()
}

func describeConstraint(constraint *types.Interface) string {
	if constraint == nil || constraint.Empty() {
		return "any"
	}

	if constraint.NumMethods() > 0 {
		return "interface-method"
	}

	if constraint.IsComparable() {
		return "comparable"
	}

	if constraint.NumEmbeddeds() > 0 {
		return "union"
	}

	return "complex"
}

func isMutableParam(mutParams []string, name, typeStr, funcName string) bool {
	if !isReferenceType(typeStr) {
		return false
	}
	if mutParams != nil {
		return slices.Contains(mutParams, name)
	}
	return looksLikeMutableParam(name, typeStr, funcName)
}

// looksLikeMutableParam returns true if the parameter is likely written into.
func looksLikeMutableParam(name, typeStr, funcName string) bool {
	if name == "dst" {
		return true
	}
	if typeStr == "Slice<byte>" || typeStr == "Slice<uint8>" {
		switch funcName {
		case "Read", "ReadAt", "ReadFull", "ReadFrom", "ReadMsgUDP", "Recv", "ReadPixels":
			return true
		}
	}
	return false
}

var constructorPrefixes = [...]string{
	"New", "Must", "Default", "Open", "Create",
	"Init", "Make", "Connect", "Dial", "Build",
	"Acquire", "Start", "With", "QueryRow",
}

// looksLikeConstructor returns true if the function name matches a constructor prefix.
func looksLikeConstructor(name string) bool {
	for _, prefix := range constructorPrefixes {
		if strings.HasPrefix(name, prefix) {
			return true
		}
	}
	return name == "Clone" || name == "Copy"
}

// looksLikeNavigationMethod returns true if a method name suggests a
// traversal or lookup that commonly returns nil.
func looksLikeNavigationMethod(name string) bool {
	switch name {
	case "Next", "Prev", "Parent", "Get", "Innermost":
		return true
	}
	return strings.Contains(name, "Lookup") || strings.Contains(name, "Find")
}

// isSelfReturning returns true if a method's single pointer return type
// matches the receiver's base type name.
func isSelfReturning(sig *types.Signature, receiverTypeName string) bool {
	results := sig.Results()
	if results.Len() != 1 || receiverTypeName == "" {
		return false
	}
	ptr, ok := results.At(0).Type().Underlying().(*types.Pointer)
	if !ok {
		return false
	}
	named, ok := ptr.Elem().(*types.Named)
	if !ok {
		return false
	}
	return named.Obj().Name() == receiverTypeName
}

// isPointerBoxingFunction returns true if a function takes a single value-type
// parameter and returns a pointer to the same type (e.g., func Bool(v bool) *bool).
func isPointerBoxingFunction(sig *types.Signature) bool {
	if sig.Params().Len() != 1 || sig.Results().Len() != 1 {
		return false
	}
	if sig.TypeParams().Len() > 0 {
		return false
	}
	param := sig.Params().At(0).Type()
	if _, isPtr := param.Underlying().(*types.Pointer); isPtr {
		return false
	}
	ptr, ok := sig.Results().At(0).Type().Underlying().(*types.Pointer)
	if !ok {
		return false
	}
	return types.Identical(ptr.Elem(), param)
}

// singlePointerReturnNamed returns the *types.Named for a signature with
// exactly one *T return, or nil.
func singlePointerReturnNamed(sig *types.Signature) *types.Named {
	results := sig.Results()
	if results.Len() != 1 {
		return nil
	}
	ptr, ok := results.At(0).Type().Underlying().(*types.Pointer)
	if !ok {
		return nil
	}
	named, ok := ptr.Elem().(*types.Named)
	if !ok {
		return nil
	}
	return named
}

// isIteratorReturnType returns true if the return type is *T where T's name
// ends with "Iterator".
func isIteratorReturnType(sig *types.Signature) bool {
	named := singlePointerReturnNamed(sig)
	return named != nil && strings.HasSuffix(named.Obj().Name(), "Iterator")
}

// isManyToOneFactory returns true if 10+ free functions in the same package
// return the same pointer type.
const (
	manyToOneFactoryThreshold      = 10
	uniformPointerMethodThreshold  = 10
	majorityPointerMethodThreshold = 20
	majorityPointerRatio           = 0.9
)

func (c *Converter) isManyToOneFactory(sig *types.Signature) bool {
	if c.manyToOneTypes == nil {
		c.analyzeManyToOneFactories()
	}
	named := singlePointerReturnNamed(sig)
	if named == nil {
		return false
	}
	return c.manyToOneTypes[named.Obj().Name()]
}

func (c *Converter) analyzeManyToOneFactories() {
	c.manyToOneTypes = make(map[string]bool)
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}

	counts := make(map[string]int)
	scope := c.pkg.Types.Scope()
	for _, name := range scope.Names() {
		obj := scope.Lookup(name)
		fn, ok := obj.(*types.Func)
		if !ok {
			continue
		}
		sig, ok := fn.Type().(*types.Signature)
		if !ok || sig.Recv() != nil {
			continue // skip methods
		}
		named := singlePointerReturnNamed(sig)
		if named == nil {
			continue
		}
		counts[named.Obj().Name()]++
	}

	for typeName, count := range counts {
		if count >= manyToOneFactoryThreshold {
			c.manyToOneTypes[typeName] = true
		}
	}
}

// hasMatchingSelfReturningMethod returns true if a free function F(args) -> *T
// has a corresponding self-returning method T.F(self, args) -> *T.
func (c *Converter) hasMatchingSelfReturningMethod(funcName string, sig *types.Signature) bool {
	named := singlePointerReturnNamed(sig)
	if named == nil {
		return false
	}

	typeName := named.Obj().Name()
	ptrMethodSet := types.NewMethodSet(types.NewPointer(named))
	for method := range ptrMethodSet.Methods() {
		if method.Obj().Name() != funcName || !method.Obj().Exported() {
			continue
		}
		methodSig, ok := method.Type().(*types.Signature)
		if !ok {
			continue
		}
		if isSelfReturning(methodSig, typeName) {
			return true
		}
	}

	return false
}

// isUniformPointerReturnType returns true if the named type has 10+ methods
// that return a single pointer to a type other than the receiver.
func (c *Converter) isUniformPointerReturnType(typeName string) bool {
	if c.uniformPointerTypes == nil {
		c.analyzeUniformPointerTypes()
	}
	return c.uniformPointerTypes[typeName]
}

func (c *Converter) analyzeUniformPointerTypes() {
	c.uniformPointerTypes = make(map[string]bool)
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}

	scope := c.pkg.Types.Scope()
	for _, name := range scope.Names() {
		obj := scope.Lookup(name)
		tn, ok := obj.(*types.TypeName)
		if !ok {
			continue
		}
		named, ok := tn.Type().(*types.Named)
		if !ok {
			continue
		}

		ptrMethodSet := types.NewMethodSet(types.NewPointer(named))
		count := 0
		var firstReturnType types.Type
		distinctTypes := false
		for method := range ptrMethodSet.Methods() {
			methodName := method.Obj().Name()
			if !method.Obj().Exported() {
				continue
			}
			sig, ok := method.Type().(*types.Signature)
			if !ok {
				continue
			}
			results := sig.Results()
			if results.Len() != 1 {
				continue
			}
			ptr, ok := results.At(0).Type().Underlying().(*types.Pointer)
			if !ok {
				continue
			}
			// Exclude self-returning methods (already handled by builder chain heuristic)
			if retNamed, ok := ptr.Elem().(*types.Named); ok {
				if retNamed.Obj().Name() == named.Obj().Name() {
					continue
				}
			}
			// Exclude Get* accessors (genuinely return nil for unset fields).
			if len(methodName) > 3 && methodName[:3] == "Get" && methodName[3] >= 'A' && methodName[3] <= 'Z' {
				continue
			}
			count++
			if firstReturnType == nil {
				firstReturnType = ptr.Elem()
			} else if !distinctTypes && !types.Identical(firstReturnType, ptr.Elem()) {
				distinctTypes = true
			}
			// Early exit once both thresholds are met
			if count >= uniformPointerMethodThreshold && distinctTypes {
				break
			}
		}

		// Require 10+ methods AND 2+ distinct return types.
		if count >= uniformPointerMethodThreshold && distinctTypes {
			c.uniformPointerTypes[named.Obj().Name()] = true
		}
	}
}

// isMajorityPointerReturnType returns true if the named type has ≥20 methods
// returning the same *T, representing >90% of single-pointer-returning methods.
func (c *Converter) isMajorityPointerReturnType(typeName string) bool {
	if c.majorityPointerTypes == nil {
		c.analyzeMajorityPointerTypes()
	}
	return c.majorityPointerTypes[typeName]
}

func (c *Converter) analyzeMajorityPointerTypes() {
	c.majorityPointerTypes = make(map[string]bool)
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}

	scope := c.pkg.Types.Scope()
	for _, name := range scope.Names() {
		obj := scope.Lookup(name)
		tn, ok := obj.(*types.TypeName)
		if !ok {
			continue
		}
		named, ok := tn.Type().(*types.Named)
		if !ok {
			continue
		}

		ptrMethodSet := types.NewMethodSet(types.NewPointer(named))
		counts := make(map[string]int) // return type name → count
		total := 0
		for method := range ptrMethodSet.Methods() {
			if !method.Obj().Exported() {
				continue
			}
			sig, ok := method.Type().(*types.Signature)
			if !ok {
				continue
			}
			results := sig.Results()
			if results.Len() != 1 {
				continue
			}
			ptr, ok := results.At(0).Type().Underlying().(*types.Pointer)
			if !ok {
				continue
			}
			retNamed, ok := ptr.Elem().(*types.Named)
			if !ok {
				continue
			}
			// Skip self-returning (already handled)
			if retNamed.Obj().Name() == named.Obj().Name() {
				continue
			}
			counts[retNamed.Obj().Name()]++
			total++
		}

		for _, count := range counts {
			if count >= majorityPointerMethodThreshold && total > 0 && float64(count)/float64(total) > majorityPointerRatio {
				c.majorityPointerTypes[named.Obj().Name()] = true
				break
			}
		}
	}
}

// bestImplementedInterface returns the most specific interface (largest method
// set; ties prefer same-package, then smaller qualified name) that the unexported
// value type `t` satisfies, or nil for none.
func (c *Converter) bestImplementedInterface(t *types.Named) *types.Named {
	if t.TypeParams().Len() > 0 {
		return nil
	}
	var best *types.Named
	for _, candidate := range c.collectInterfaceCandidates() {
		if !types.Implements(t, candidate.Underlying().(*types.Interface)) {
			continue
		}
		if !c.interfaceRepresentable(candidate) {
			continue
		}
		if best == nil || c.moreSpecificInterface(candidate, best) {
			best = candidate
		}
	}
	return best
}

// interfaceRepresentable reports whether bindgen can emit `named` as a Lisette
// interface, so a marker var is never typed by one that would dangle or be
// skipped.
func (c *Converter) interfaceRepresentable(named *types.Named) bool {
	if c.ifaceRepresentable == nil {
		c.ifaceRepresentable = make(map[*types.Named]bool)
		c.ifaceProbing = make(map[*types.Named]bool)
	}
	if verdict, ok := c.ifaceRepresentable[named]; ok {
		return verdict
	}
	if c.ifaceProbing[named] {
		return true
	}
	c.ifaceProbing[named] = true
	defer delete(c.ifaceProbing, named)

	iface := named.Underlying().(*types.Interface)
	synthMark := c.synthMark()
	savedPkgs := make(ExternalPkgs, len(c.externalPkgs))
	maps.Copy(savedPkgs, c.externalPkgs)
	_, representable := c.extractInterfaceMethods(iface, named.Obj().Name())
	c.rollbackSynth(synthMark)
	c.externalPkgs = savedPkgs
	c.ifaceRepresentable[named] = representable
	return representable
}

func (c *Converter) moreSpecificInterface(a, b *types.Named) bool {
	am := a.Underlying().(*types.Interface).NumMethods()
	bm := b.Underlying().(*types.Interface).NumMethods()
	if am != bm {
		return am > bm
	}
	aSame := a.Obj().Pkg().Path() == c.currentPkgPath
	bSame := b.Obj().Pkg().Path() == c.currentPkgPath
	if aSame != bSame {
		return aSame
	}
	return qualifiedName(a) < qualifiedName(b)
}

func qualifiedName(named *types.Named) string {
	return named.Obj().Pkg().Path() + "." + named.Obj().Name()
}

func (c *Converter) collectInterfaceCandidates() []*types.Named {
	if c.ifaceCandidates != nil {
		return c.ifaceCandidates
	}
	candidates := []*types.Named{}

	collect := func(scope *types.Scope) {
		for _, name := range scope.Names() {
			typeName, ok := scope.Lookup(name).(*types.TypeName)
			if !ok || !typeName.Exported() || typeName.Pkg() == nil {
				continue
			}
			if extract.IsInternalPackagePath(typeName.Pkg().Path()) {
				continue
			}
			named, ok := typeName.Type().(*types.Named)
			if !ok || named.TypeParams().Len() > 0 {
				continue
			}
			// IsMethodSet excludes constraint interfaces (unions, `~T` terms).
			iface, ok := named.Underlying().(*types.Interface)
			if !ok || iface.NumMethods() == 0 || !iface.IsMethodSet() {
				continue
			}
			candidates = append(candidates, named)
		}
	}

	if c.pkg != nil && c.pkg.Types != nil {
		collect(c.pkg.Types.Scope())
		for _, imported := range c.pkg.Imports {
			if imported != nil && imported.Types != nil {
				collect(imported.Types.Scope())
			}
		}
	}

	c.ifaceCandidates = candidates
	return c.ifaceCandidates
}

// hasReachableUnexportedType reports whether any exported declaration in the
// current package surfaces a value of the given unexported named type.
func (c *Converter) hasReachableUnexportedType(named *types.Named) bool {
	if c.reachableUnexportedTypes == nil {
		c.computeReachableUnexportedTypes()
	}
	return c.reachableUnexportedTypes[named.Obj().Name()]
}

func (c *Converter) computeReachableUnexportedTypes() {
	c.reachableUnexportedTypes = make(map[string]bool)
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}
	seen := make(map[types.Type]bool)
	scope := c.pkg.Types.Scope()
	for _, name := range scope.Names() {
		obj := scope.Lookup(name)
		if !obj.Exported() {
			continue
		}
		switch o := obj.(type) {
		case *types.Const, *types.Var, *types.Func:
			c.markUnexportedNamesIn(o.Type(), seen)
		case *types.TypeName:
			named, ok := o.Type().(*types.Named)
			if !ok {
				continue
			}
			for method := range types.NewMethodSet(types.NewPointer(named)).Methods() {
				if method.Obj().Exported() {
					c.markUnexportedNamesIn(method.Type(), seen)
				}
			}
			if s, ok := named.Underlying().(*types.Struct); ok {
				for i := 0; i < s.NumFields(); i++ {
					if f := s.Field(i); f.Exported() {
						c.markUnexportedNamesIn(f.Type(), seen)
					}
				}
			}
		}
	}
}

func (c *Converter) isOpaqueHandleStruct(named *types.Named) bool {
	obj := named.Obj()
	if obj == nil || obj.Exported() {
		return false
	}
	if obj.Pkg() == nil || obj.Pkg().Path() != c.currentPkgPath {
		return false
	}
	if named.TypeParams().Len() > 0 || named.TypeArgs().Len() > 0 {
		return false
	}
	s, ok := named.Underlying().(*types.Struct)
	if !ok || s.NumFields() == 0 {
		return false
	}
	if namedImplementsError(named) {
		return false
	}
	return c.bestImplementedInterface(named) == nil
}

func (c *Converter) computeDirectProducers() {
	c.directProducers = make(map[string]bool)
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}
	scope := c.pkg.Types.Scope()
	for _, name := range scope.Names() {
		obj := scope.Lookup(name)
		if obj == nil || !obj.Exported() {
			continue
		}
		v, ok := obj.(*types.Var)
		if !ok {
			continue
		}
		named, ok := types.Unalias(v.Type()).(*types.Named)
		if !ok {
			continue
		}
		if c.isOpaqueHandleStruct(named) {
			c.directProducers[named.Obj().Name()] = true
		}
	}
}

// directHandle resolves t to an eligible opaque handle only when t is a bare
// *types.Named, so nested occurrences (`[]chest`, `func(chest)`, `...chest`)
// keep skipping through the normal conversion path.
func (c *Converter) directHandle(t types.Type) (*types.Named, bool) {
	if c.directProducers == nil {
		c.computeDirectProducers()
	}
	named, ok := types.Unalias(t).(*types.Named)
	if !ok {
		return nil, false
	}
	obj := named.Obj()
	if obj.Pkg() == nil || obj.Pkg().Path() != c.currentPkgPath {
		return nil, false
	}
	if !c.directProducers[obj.Name()] {
		return nil, false
	}
	return named, true
}

func (c *Converter) directHandleIfEligible(t types.Type, directEligible bool) (*types.Named, bool) {
	if !directEligible {
		return nil, false
	}
	return c.directHandle(t)
}

func (c *Converter) OpaqueHandles() []ConvertResult {
	if c.directProducers == nil {
		c.computeDirectProducers()
	}
	out := make([]ConvertResult, 0, len(c.directProducers))
	for name := range c.directProducers {
		out = append(out, ConvertResult{
			Name:           name,
			Kind:           extract.ExportType,
			UnexportedType: true,
		})
	}
	slices.SortFunc(out, func(a, b ConvertResult) int { return strings.Compare(a.Name, b.Name) })
	return out
}

// markUnexportedNamesIn walks `t` and marks any unexported named types from
// the current package as reachable. Recurses through wrapper types
// (Pointer/Slice/Array/Map/Chan) and through Signature results — the latter
// so that `func() level` counts as evidence that `level` is reachable.
func (c *Converter) markUnexportedNamesIn(t types.Type, seen map[types.Type]bool) {
	if t == nil || seen[t] {
		return
	}
	seen[t] = true

	switch t := t.(type) {
	case *types.Named:
		obj := t.Obj()
		if obj.Pkg() != nil && obj.Pkg().Path() == c.currentPkgPath && !obj.Exported() {
			c.reachableUnexportedTypes[obj.Name()] = true
		}
	case *types.Pointer:
		c.markUnexportedNamesIn(t.Elem(), seen)
	case *types.Slice:
		c.markUnexportedNamesIn(t.Elem(), seen)
	case *types.Array:
		c.markUnexportedNamesIn(t.Elem(), seen)
	case *types.Map:
		c.markUnexportedNamesIn(t.Key(), seen)
		c.markUnexportedNamesIn(t.Elem(), seen)
	case *types.Chan:
		c.markUnexportedNamesIn(t.Elem(), seen)
	case *types.Signature:
		results := t.Results()
		for i := 0; i < results.Len(); i++ {
			c.markUnexportedNamesIn(results.At(i).Type(), seen)
		}
	}
}

func sealQualifier(p *types.Package) string {
	if p == nil {
		return ""
	}
	return p.Path()
}

// sealIdentity is an unexported method's package-qualified identity: declaring
// package, name, and signature (receiver excluded). A seal and a method that
// implements it share it; a different signature or package does not.
func sealIdentity(pkgPath, name string, sig *types.Signature) string {
	var b strings.Builder
	b.WriteString(pkgPath)
	b.WriteString(".")
	b.WriteString(name)
	b.WriteString("(")
	if sig != nil {
		params := sig.Params()
		for i := 0; i < params.Len(); i++ {
			if i > 0 {
				b.WriteString(",")
			}
			if sig.Variadic() && i == params.Len()-1 {
				b.WriteString("...")
			}
			b.WriteString(types.TypeString(params.At(i).Type(), sealQualifier))
		}
	}
	b.WriteString(")")
	if sig != nil && sig.Results().Len() > 0 {
		b.WriteString(" ")
		results := sig.Results()
		for i := 0; i < results.Len(); i++ {
			if i > 0 {
				b.WriteString(",")
			}
			b.WriteString(types.TypeString(results.At(i).Type(), sealQualifier))
		}
	}
	return b.String()
}

// extractInterfaceMethods walks a Go interface's method set and converts each to
// a Lisette InterfaceMethod. The second return value is false when an embedded
// union or an unrepresentable exported param/return type is encountered.
// Unexported methods are recorded by their seal identity only.
func (c *Converter) extractInterfaceMethods(_interface *types.Interface, typeName string) ([]InterfaceMethod, bool) {
	if _interface.NumEmbeddeds() > 0 {
		for embedded := range _interface.EmbeddedTypes() {
			if _, isUnion := embedded.(*types.Union); isUnion {
				return nil, false
			}
		}
	}

	var methods []InterfaceMethod

	for method := range _interface.Methods() {
		if !method.Exported() {
			sig, _ := method.Type().(*types.Signature)
			methods = append(methods, InterfaceMethod{
				Name:   method.Name(),
				SealId: sealIdentity(method.Pkg().Path(), method.Name(), sig),
			})
			continue
		}

		signature, ok := method.Type().(*types.Signature)
		if !ok {
			return nil, false
		}

		qualifiedName := typeName + "." + method.Name()
		params, skip := c.convertParams(signature, qualifiedName, method.Name(), nil, false)
		if skip != nil {
			return nil, false
		}

		returnType := ReturnsToLisette(signature, c, qualifiedName)
		if returnType.SkipReason != nil {
			return nil, false
		}

		// Interface methods are contracts; any nilable return permits nil.
		if isSingleNilableResult(signature) && !returnType.IsDirectError &&
			(c.cfg == nil || !c.cfg.IsNonNilableReturn(c.currentPkgPath, qualifiedName)) {
			returnType.LisetteType = optionOf(returnType.LisetteType)
		}

		methods = append(methods, InterfaceMethod{
			Name:       method.Name(),
			Params:     params,
			ReturnType: returnType.LisetteType,
			CommaOk:    returnType.CommaOk,
		})
	}

	return methods, true
}
