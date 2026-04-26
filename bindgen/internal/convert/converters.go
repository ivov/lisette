package convert

import (
	"fmt"
	"go/ast"
	"go/constant"
	"go/token"
	"go/types"
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
	return strings.HasPrefix(typeStr, "Slice<") || strings.HasPrefix(typeStr, "Map<")
}

func (c *Converter) convertFunction(result *ConvertResult, symbolExport extract.SymbolExport) {
	signature, ok := symbolExport.GoType.(*types.Signature)
	if !ok {
		result.SkipReason = &SkipReason{Code: "invalid-signature", Message: "not a function signature"}
		return
	}

	// Scan first: param/return processing must observe `S ~[]E` substitutions.
	typeParams, substitutions, skip := collectTypeParams(signature.TypeParams(), false, c)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.TypeParams = typeParams

	prevSubs := c.typeParamSubstitutions
	c.typeParamSubstitutions = substitutions
	defer func() { c.typeParamSubstitutions = prevSubs }()

	mutParams := c.cfg.MutatingParams(c.currentPkgPath, result.Name)

	params := signature.Params()
	for i := 0; i < params.Len(); i++ {
		param := params.At(i)
		paramType := ToLisette(param.Type(), c)
		if paramType.SkipReason != nil {
			result.SkipReason = paramType.SkipReason
			return
		}

		typeStr := paramType.LisetteType
		if signature.Variadic() && i == params.Len()-1 {
			typeStr = sliceToVarArgs(typeStr)
		}

		name := param.Name()
		if name == "" {
			name = fmt.Sprintf("arg%d", i)
		}
		name = sanitizeParamName(name)

		result.Params = append(result.Params, FunctionParameter{
			Name:    name,
			Type:    typeStr,
			Mutable: isMutableParam(mutParams, name, typeStr, result.Name),
		})
	}

	returnType := ReturnsToLisette(signature, c, result.Name)
	if returnType.LisetteType != "" {
		result.ReturnType = returnType.LisetteType
	} else if returnType.SkipReason != nil {
		result.ReturnType = "Unknown"
	}
	result.CommaOk = returnType.CommaOk
	result.ArrayReturn = returnType.ArrayReturn
	c.applySentinelInt(result, result.Name)

	isSinglePointerReturn := isSinglePointerResult(signature)
	if isSinglePointerReturn && returnType.IsDirectError {
		isSinglePointerReturn = false
	}
	forceNonNilable := false
	forceNilable := c.cfg != nil && c.cfg.ShouldWrapNilableReturn(c.currentPkgPath, result.Name)
	if isSinglePointerReturn && looksLikeConstructor(result.Name) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn && isPointerBoxingFunction(signature) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn && isIteratorReturnType(signature) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn && c.isManyToOneFactory(signature) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn && c.hasMatchingSelfReturningMethod(result.Name, signature) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn {
		forceNonNilable = c.isProvenNonNilReturn(symbolExport.Obj)
	}
	if !forceNonNilable {
		forceNonNilable = c.cfg != nil && c.cfg.IsNonNilableReturn(c.currentPkgPath, result.Name)
	}
	if (isSinglePointerReturn && !forceNonNilable) || (forceNilable && !returnType.NilableReturnApplied) {
		result.ReturnType = fmt.Sprintf("Option<%s>", result.ReturnType)
	}
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
			isPointerReceiver := symbolExport.NeedsPointerReceiver

			recvLisetteType := typeName
			if isPointerReceiver {
				recvLisetteType = fmt.Sprintf("Ref<%s>", typeName)
			}

			result.Receiver = &Receiver{
				Name:         symbolExport.ReceiverVariable.Name(),
				Type:         recvLisetteType,
				IsPointer:    isPointerReceiver,
				BaseTypeName: typeName,
				TypeParams:   typeParams,
			}
		} else {
			recvType := ToLisette(symbolExport.ReceiverVariable.Type(), c)
			if recvType.SkipReason != nil {
				result.SkipReason = recvType.SkipReason
				return
			}

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

			result.Receiver = &Receiver{
				Name:         symbolExport.ReceiverVariable.Name(),
				Type:         recvType.LisetteType,
				IsPointer:    isPointerReceiver,
				BaseTypeName: typeName,
				TypeParams:   typeParams,
			}
		}
	}

	qualifiedName := result.Name
	if result.Receiver != nil && result.Receiver.BaseTypeName != "" {
		qualifiedName = result.Receiver.BaseTypeName + "." + result.Name
	}

	mutParams := c.cfg.MutatingParams(c.currentPkgPath, qualifiedName)

	params := signature.Params()
	for i := 0; i < params.Len(); i++ {
		param := params.At(i)
		paramType := ToLisette(param.Type(), c)
		if paramType.SkipReason != nil {
			result.SkipReason = paramType.SkipReason
			return
		}

		typeStr := paramType.LisetteType
		if signature.Variadic() && i == params.Len()-1 {
			typeStr = sliceToVarArgs(typeStr)
		}

		name := param.Name()
		if name == "" {
			name = fmt.Sprintf("arg%d", i)
		}
		name = sanitizeParamName(name)

		result.Params = append(result.Params, FunctionParameter{
			Name:    name,
			Type:    typeStr,
			Mutable: isMutableParam(mutParams, name, typeStr, result.Name),
		})
	}

	returnType := ReturnsToLisette(signature, c, qualifiedName)
	if returnType.LisetteType != "" {
		result.ReturnType = returnType.LisetteType
	} else if returnType.SkipReason != nil {
		result.ReturnType = "Unknown"
	}
	result.CommaOk = returnType.CommaOk
	result.ArrayReturn = returnType.ArrayReturn
	c.applySentinelInt(result, qualifiedName)

	isSinglePointerReturn := isSinglePointerResult(signature)
	if isSinglePointerReturn && returnType.IsDirectError {
		isSinglePointerReturn = false
	}
	forceNonNilable := isSinglePointerReturn && looksLikeConstructor(result.Name)
	if !forceNonNilable && isSinglePointerReturn && result.Receiver != nil && !looksLikeNavigationMethod(result.Name) {
		if isSelfReturning(signature, result.Receiver.BaseTypeName) ||
			c.isUniformPointerReturnType(result.Receiver.BaseTypeName) ||
			c.isMajorityPointerReturnType(result.Receiver.BaseTypeName) {
			forceNonNilable = true
		}
	}
	if !forceNonNilable && isSinglePointerReturn && symbolExport.IsPromoted && symbolExport.OriginalTypeName != "" {
		if isSelfReturning(signature, symbolExport.OriginalTypeName) {
			forceNonNilable = true
		}
	}
	if !forceNonNilable && isSinglePointerReturn && isIteratorReturnType(signature) {
		forceNonNilable = true
	}
	if !forceNonNilable && isSinglePointerReturn {
		forceNonNilable = c.isProvenNonNilReturn(symbolExport.Obj)
	}
	if !forceNonNilable {
		forceNonNilable = c.cfg != nil && c.cfg.IsNonNilableReturn(c.currentPkgPath, qualifiedName)
	}
	if !forceNonNilable && symbolExport.IsPromoted && symbolExport.OriginalTypeName != "" {
		originalQualified := symbolExport.OriginalTypeName + "." + result.Name
		forceNonNilable = c.cfg != nil && c.cfg.IsNonNilableReturn(symbolExport.OriginalPkgPath, originalQualified)
	}
	methodForceNilable := c.cfg != nil && c.cfg.ShouldWrapNilableReturn(c.currentPkgPath, qualifiedName)
	if !methodForceNilable && symbolExport.IsPromoted && symbolExport.OriginalTypeName != "" {
		originalQualified := symbolExport.OriginalTypeName + "." + result.Name
		methodForceNilable = c.cfg != nil && c.cfg.ShouldWrapNilableReturn(symbolExport.OriginalPkgPath, originalQualified)
	}
	if (isSinglePointerReturn && !forceNonNilable) || (methodForceNilable && !returnType.NilableReturnApplied) {
		result.ReturnType = fmt.Sprintf("Option<%s>", result.ReturnType)
	}

	if symbolExport.BaseType != nil {
		_, _, skip := collectTypeParams(symbolExport.BaseType.TypeParams(), false, c)
		if skip != nil {
			result.SkipReason = skip
			return
		}
	}

	methodSpecs, _, skip := collectTypeParams(signature.TypeParams(), false, c)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.TypeParams = methodSpecs
}

func (c *Converter) convertType(result *ConvertResult, exp extract.SymbolExport) {
	if alias, ok := exp.GoType.(*types.Alias); ok {
		rhs := alias.Rhs()
		t := ToLisette(rhs, c)
		if t.SkipReason != nil {
			result.SkipReason = withOpaqueType(t.SkipReason)
			return
		}
		result.LisetteType = t.LisetteType
		result.IsTypeAlias = true
		return
	}

	named, ok := exp.GoType.(*types.Named)
	if !ok {
		result.SkipReason = &SkipReason{Code: "not-named-type", Message: "expected named type"}
		return
	}

	typeParams, _, skip := collectTypeParams(named.TypeParams(), true, c)
	if skip != nil {
		result.SkipReason = skip
		return
	}
	result.TypeParams = typeParams

	underlying := named.Underlying()

	switch u := underlying.(type) {
	case *types.Struct:
		for field := range u.Fields() {
			if !field.Exported() {
				continue
			}

			fieldType := ToLisetteNilable(field.Type(), c)
			if fieldType.SkipReason != nil {
				continue
			}

			result.Fields = append(result.Fields, StructField{
				Name: field.Name(),
				Type: fieldType.LisetteType,
			})
		}

	case *types.Interface:
		if isErrorInterface(u) {
			result.LisetteType = "error"
		} else if u.Empty() {
			result.IsInterface = true
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
		result.SkipReason = t.SkipReason
		return
	}
	result.LisetteType = t.LisetteType
}

func (c *Converter) convertVariable(result *ConvertResult, exp extract.SymbolExport) {
	t := ToLisette(exp.GoType, c)
	if t.SkipReason != nil {
		result.SkipReason = t.SkipReason
		return
	}
	result.LisetteType = t.LisetteType

	isNilable := isNilableGoType(exp.GoType)
	forceNonNilable := c.cfg != nil && (c.cfg.IsNonNilableVar(c.currentPkgPath, result.Name) || c.cfg.IsNonNilableReturn(c.currentPkgPath, result.Name))
	if !forceNonNilable && isNilable {
		forceNonNilable = c.isProvenNonNilVar(exp.Obj)
	}
	if isNilable && !forceNonNilable {
		result.LisetteType = fmt.Sprintf("Option<%s>", result.LisetteType)
	}
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
			return literal
		}
	}

	return ""
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

func sliceToVarArgs(typeStr string) string {
	if strings.HasPrefix(typeStr, "Slice<") && strings.HasSuffix(typeStr, ">") {
		elemType := typeStr[6 : len(typeStr)-1]
		return fmt.Sprintf("VarArgs<%s>", elemType)
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

// `S ~[]E` shapes go into substitutions (caller rewrites `S` to `Slice<E>`)
// rather than into specs. Recognized bounds register their imports on conv.
func collectTypeParams(
	typeParams *types.TypeParamList,
	emitOpaque bool,
	conv *Converter,
) (specs TypeParamSpecs, substitutions map[string]string, skip *SkipReason) {
	if typeParams == nil {
		return nil, nil, nil
	}

	for tp := range typeParams.TypeParams() {
		name := tp.Obj().Name()
		constraint := tp.Constraint()

		if elemName, ok := recognizeSliceShape(constraint); ok {
			if substitutions == nil {
				substitutions = make(map[string]string)
			}
			substitutions[name] = fmt.Sprintf("Slice<%s>", elemName)
			continue
		}

		if isAnyConstraint(constraint) {
			specs = append(specs, TypeParamSpec{Name: name})
			continue
		}

		var currentPkg string
		if conv != nil {
			currentPkg = conv.currentPkgPath
		}
		if boundExpr, ok, imports := recognizeBound(constraint, currentPkg); ok {
			for _, path := range imports {
				if conv != nil {
					conv.trackExternalPkg(path, path)
				}
			}
			specs = append(specs, TypeParamSpec{Name: name, Bound: boundExpr})
			continue
		}

		iface, _ := constraint.Underlying().(*types.Interface)
		return nil, nil, &SkipReason{
			Code:           "constraint:" + describeConstraint(iface),
			Message:        fmt.Sprintf("type constraint %s cannot be represented", name),
			EmitOpaqueType: emitOpaque,
		}
	}
	return specs, substitutions, nil
}

func extractReceiverTypeParams(named *types.Named, conv *Converter) TypeParamSpecs {
	origin := named.Origin()
	typeParams := origin.TypeParams()
	if typeParams == nil || typeParams.Len() == 0 {
		return nil
	}

	specs, _, skip := collectTypeParams(typeParams, false, conv)
	if skip != nil {
		// Base type emits the skip; impl block falls back to bare names.
		var fallback TypeParamSpecs
		for tp := range typeParams.TypeParams() {
			fallback = append(fallback, TypeParamSpec{Name: tp.Obj().Name()})
		}
		return fallback
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

	if constraint.IsComparable() {
		return "comparable"
	}

	if constraint.NumMethods() > 0 {
		return "interface-method"
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
	if typeStr == "Slice<uint8>" {
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
		if count >= 10 {
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
			if count >= 10 && distinctTypes {
				break
			}
		}

		// Require 10+ methods AND 2+ distinct return types.
		if count >= 10 && distinctTypes {
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
			if count >= 20 && total > 0 && float64(count)/float64(total) > 0.9 {
				c.majorityPointerTypes[named.Obj().Name()] = true
				break
			}
		}
	}
}

// extractInterfaceMethods walks a Go interface's exported methods and converts
// each to a Lisette InterfaceMethod. The second return value reports whether
// the interface is representable at all: false when an embedded union or an
// unrepresentable param/return type is encountered, true otherwise. A true
// return with an empty slice means the interface has no exported methods
// (e.g. empty interface or all methods unexported) and should be emitted as
// `pub interface Name {}`.
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
			continue
		}

		signature, ok := method.Type().(*types.Signature)
		if !ok {
			return nil, false
		}

		mutParams := c.cfg.MutatingParams(c.currentPkgPath, typeName+"."+method.Name())

		var params []FunctionParameter
		for j := 0; j < signature.Params().Len(); j++ {
			param := signature.Params().At(j)
			paramType := ToLisette(param.Type(), c)
			if paramType.SkipReason != nil {
				return nil, false
			}

			name := param.Name()
			if name == "" {
				name = fmt.Sprintf("arg%d", j)
			}
			name = sanitizeParamName(name)

			params = append(params, FunctionParameter{
				Name:    name,
				Type:    paramType.LisetteType,
				Mutable: isMutableParam(mutParams, name, paramType.LisetteType, method.Name()),
			})
		}

		returnType := ReturnsToLisette(signature, c, typeName+"."+method.Name())
		if returnType.SkipReason != nil {
			return nil, false
		}

		methods = append(methods, InterfaceMethod{
			Name:        method.Name(),
			Params:      params,
			ReturnType:  returnType.LisetteType,
			CommaOk:     returnType.CommaOk,
			ArrayReturn: returnType.ArrayReturn,
		})
	}

	return methods, true
}
