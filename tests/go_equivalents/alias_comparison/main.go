package main

import (
	"bufio"
	"bytes"
	"fmt"
	"go/ast"
	"go/parser"
	"go/printer"
	"go/token"
	"os"
	"path"
	"reflect"
	"strconv"
	"strings"
)

const aliasName = "MmAlias"

type comparison struct {
	target        ast.Expr
	fileSets      [2]*token.FileSet
	context       [2]ast.Node
	originalShown string
	variantShown  string
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: alias_comparison <pairs file>")
		os.Exit(2)
	}
	pairs, err := os.Open(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	defer pairs.Close()
	scanner := bufio.NewScanner(pairs)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	output := bufio.NewWriter(os.Stdout)
	defer output.Flush()
	for scanner.Scan() {
		fields := strings.Split(scanner.Text(), "\t")
		if len(fields) != 3 {
			fmt.Fprintf(os.Stderr, "bad input line: %q\n", scanner.Text())
			os.Exit(2)
		}
		original, variant, differs := compareFiles(fields[1], fields[2])
		if differs {
			fmt.Fprintf(output, "%s\t%s\t%s\n", fields[0], original, variant)
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
}

func compareFiles(originalPath, variantPath string) (string, string, bool) {
	originalSet := token.NewFileSet()
	original, err := parser.ParseFile(originalSet, originalPath, nil, parser.SkipObjectResolution)
	if err != nil {
		return "(original does not parse)", err.Error(), true
	}
	variantSet := token.NewFileSet()
	variant, err := parser.ParseFile(variantSet, variantPath, nil, parser.SkipObjectResolution)
	if err != nil {
		return "(variant does not parse)", err.Error(), true
	}
	target, variantDecls := removeAlias(variant.Decls)
	if target == nil {
		return "(no alias)", "(variant has no `type " + aliasName + " = ...`)", true
	}
	if original, variant, differs := compareImports(original.Imports, variant.Imports, target); differs {
		return original, variant, true
	}
	state := &comparison{target: target, fileSets: [2]*token.FileSet{originalSet, variantSet}}
	originalDecls := withoutImports(original.Decls)
	variantDecls = withoutImports(variantDecls)
	for index := 0; index < len(originalDecls) || index < len(variantDecls); index++ {
		if index >= len(originalDecls) {
			return "(end of file)", firstLine(variantSet, variantDecls[index]), true
		}
		if index >= len(variantDecls) {
			return firstLine(originalSet, originalDecls[index]), "(end of file)", true
		}
		state.context = [2]ast.Node{originalDecls[index], variantDecls[index]}
		if !state.equal(reflect.ValueOf(originalDecls[index]), reflect.ValueOf(variantDecls[index])) {
			return state.originalShown, state.variantShown, true
		}
	}
	return "", "", false
}

func removeAlias(decls []ast.Decl) (ast.Expr, []ast.Decl) {
	for index, decl := range decls {
		general, ok := decl.(*ast.GenDecl)
		if !ok || general.Tok != token.TYPE || len(general.Specs) != 1 {
			continue
		}
		spec := general.Specs[0].(*ast.TypeSpec)
		if spec.Name.Name == aliasName && spec.Assign.IsValid() {
			rest := append(append([]ast.Decl{}, decls[:index]...), decls[index+1:]...)
			return spec.Type, rest
		}
	}
	return nil, decls
}

func withoutImports(decls []ast.Decl) []ast.Decl {
	var kept []ast.Decl
	for _, decl := range decls {
		if general, ok := decl.(*ast.GenDecl); ok && general.Tok == token.IMPORT {
			continue
		}
		kept = append(kept, decl)
	}
	return kept
}

func compareImports(original, variant []*ast.ImportSpec, target ast.Expr) (string, string, bool) {
	originalImports := importLines(original)
	variantImports := importLines(variant)
	qualifiers := targetQualifiers(target)
	for line := range originalImports {
		if !variantImports[line] {
			return line, "(import missing)", true
		}
	}
	for line := range variantImports {
		if !originalImports[line] && !qualifiers[importQualifier(line)] {
			return "(import missing)", line, true
		}
	}
	return "", "", false
}

func importLines(specs []*ast.ImportSpec) map[string]bool {
	lines := map[string]bool{}
	for _, spec := range specs {
		lines[importLine(spec)] = true
	}
	return lines
}

func importLine(spec *ast.ImportSpec) string {
	if spec.Name != nil {
		return spec.Name.Name + " " + spec.Path.Value
	}
	return spec.Path.Value
}

func importQualifier(line string) string {
	if name, _, found := strings.Cut(line, " "); found {
		return name
	}
	unquoted, err := strconv.Unquote(line)
	if err != nil {
		return line
	}
	return path.Base(unquoted)
}

func targetQualifiers(target ast.Expr) map[string]bool {
	qualifiers := map[string]bool{}
	ast.Inspect(target, func(node ast.Node) bool {
		if selector, ok := node.(*ast.SelectorExpr); ok {
			if identifier, ok := selector.X.(*ast.Ident); ok {
				qualifiers[identifier.Name] = true
			}
		}
		return true
	})
	return qualifiers
}

var (
	positionType     = reflect.TypeOf(token.Pos(0))
	commentGroupType = reflect.TypeOf((*ast.CommentGroup)(nil))
	fieldListType    = reflect.TypeOf((*ast.FieldList)(nil))
	identifierType   = reflect.TypeOf((*ast.Ident)(nil))
	parenthesesType  = reflect.TypeOf((*ast.ParenExpr)(nil))
)

func (state *comparison) equal(original, variant reflect.Value) bool {
	original = state.unwrap(original, false)
	variant = state.unwrap(variant, true)
	if original.Kind() != variant.Kind() || original.Type() != variant.Type() {
		return state.differ()
	}
	switch original.Kind() {
	case reflect.Interface:
		return true
	case reflect.Pointer:
		if original.IsNil() || variant.IsNil() {
			return original.IsNil() == variant.IsNil() || state.differ()
		}
		if original.Type() == fieldListType {
			return state.equalFields(original.Interface().(*ast.FieldList), variant.Interface().(*ast.FieldList))
		}
		restore := state.enter(original, variant)
		defer restore()
		return state.equal(original.Elem(), variant.Elem())
	case reflect.Struct:
		for index := 0; index < original.NumField(); index++ {
			if !state.equalField(original.Field(index), variant.Field(index)) {
				return false
			}
		}
		return true
	case reflect.Slice:
		if original.Len() != variant.Len() {
			return state.differ()
		}
		for index := 0; index < original.Len(); index++ {
			if !state.equal(original.Index(index), variant.Index(index)) {
				return false
			}
		}
		return true
	case reflect.String:
		return original.String() == variant.String() || state.differ()
	case reflect.Bool:
		return original.Bool() == variant.Bool() || state.differ()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return original.Int() == variant.Int() || state.differ()
	default:
		panic(fmt.Sprintf("unexpected kind %s", original.Kind()))
	}
}

func (state *comparison) equalField(original, variant reflect.Value) bool {
	switch original.Type() {
	case positionType:
		return token.Pos(original.Int()).IsValid() == token.Pos(variant.Int()).IsValid() || state.differ()
	case commentGroupType:
		return true
	}
	return state.equal(original, variant)
}

func (state *comparison) unwrap(value reflect.Value, isVariant bool) reflect.Value {
	for {
		if value.Kind() == reflect.Interface {
			if value.IsNil() {
				return value
			}
			value = value.Elem()
			continue
		}
		if value.Type() == parenthesesType && !value.IsNil() {
			value = reflect.ValueOf(value.Interface().(*ast.ParenExpr).X)
			continue
		}
		if isVariant && value.Type() == identifierType && !value.IsNil() &&
			value.Interface().(*ast.Ident).Name == aliasName {
			value = reflect.ValueOf(state.target)
			continue
		}
		return value
	}
}

type field struct {
	Name  *ast.Ident
	Value ast.Expr
	Tag   *ast.BasicLit
}

func flatten(list *ast.FieldList) []field {
	var fields []field
	for _, entry := range list.List {
		if len(entry.Names) == 0 {
			fields = append(fields, field{nil, entry.Type, entry.Tag})
		}
		for _, name := range entry.Names {
			fields = append(fields, field{name, entry.Type, entry.Tag})
		}
	}
	return fields
}

func (state *comparison) equalFields(original, variant *ast.FieldList) bool {
	originalFields := flatten(original)
	variantFields := flatten(variant)
	if len(originalFields) != len(variantFields) {
		return state.differ()
	}
	for index := range originalFields {
		if !state.equal(reflect.ValueOf(originalFields[index]), reflect.ValueOf(variantFields[index])) {
			return false
		}
	}
	return true
}

func (state *comparison) enter(original, variant reflect.Value) func() {
	originalNode, isNode := original.Interface().(ast.Node)
	if !isNode || !isLineNode(originalNode) {
		return func() {}
	}
	previous := state.context
	state.context = [2]ast.Node{originalNode, variant.Interface().(ast.Node)}
	return func() { state.context = previous }
}

func isLineNode(node ast.Node) bool {
	switch node.(type) {
	case ast.Stmt, ast.Decl, ast.Spec:
		return true
	}
	return false
}

func (state *comparison) differ() bool {
	state.originalShown = firstLine(state.fileSets[0], state.context[0])
	state.variantShown = firstLine(state.fileSets[1], state.context[1])
	return false
}

func firstLine(fset *token.FileSet, node ast.Node) string {
	var text bytes.Buffer
	if err := printer.Fprint(&text, fset, node); err != nil {
		return err.Error()
	}
	line, _, _ := strings.Cut(text.String(), "\n")
	return strings.TrimSpace(line)
}
