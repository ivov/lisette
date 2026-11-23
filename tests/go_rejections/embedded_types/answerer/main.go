// Command embed_go_answerer type-checks a generated Go package with go/types and
// answers selector and satisfaction questions (LookupFieldOrMethod, Implements,
// AssignableTo). Protocol: a JSON Request on stdin, a JSON Response on stdout.
package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/importer"
	"go/parser"
	"go/token"
	"go/types"
	"os"
)

type Request struct {
	GoSource  string     `json:"goSource"`
	Questions []Question `json:"questions"`
}

type Question struct {
	Kind      string `json:"kind"` // "selector" | "satisfies"
	Root      string `json:"root"`
	Member    string `json:"member"`
	TypeName  string `json:"typeName"`
	Interface string `json:"interface"`
}

type Response struct {
	// FatalError is set only when the package could not be parsed at all; a
	// type error (e.g. a deliberate duplicate-method interface) is reported in
	// TypeErrors and does not stop question answering.
	FatalError string   `json:"fatalError,omitempty"`
	TypeErrors []string `json:"typeErrors,omitempty"`
	Results    []Answer `json:"results"`
}

type Answer struct {
	Kind string `json:"kind"`

	// selector
	Resolves      bool           `json:"resolves"`
	Ambiguous     bool           `json:"ambiguous"`
	MemberKind    string         `json:"memberKind,omitempty"`
	DeclaringType string         `json:"declaringType,omitempty"`
	Depth         int            `json:"depth"`
	Indirect      bool           `json:"indirect"`
	ResolvedType  *CanonicalType `json:"resolvedType,omitempty"`

	// satisfies
	SatisfiesValue   bool `json:"satisfiesValue"`
	SatisfiesPointer bool `json:"satisfiesPointer"`
	Assignable       bool `json:"assignable"`
}

// CanonicalType is a language-neutral type token both sides lower into, so the Rust
// comparator never string-compares Go and Lisette spellings.
type CanonicalType struct {
	Kind       string           `json:"kind"` // "basic"|"named"|"ref"|"slice"|"func"|"other"
	Name       string           `json:"name,omitempty"`
	Element    *CanonicalType   `json:"element,omitempty"`
	Parameters []*CanonicalType `json:"parameters,omitempty"`
	ReturnType *CanonicalType   `json:"returnType,omitempty"`
}

func main() {
	var req Request
	if err := json.NewDecoder(os.Stdin).Decode(&req); err != nil {
		writeResponse(Response{FatalError: fmt.Sprintf("decode request: %v", err)})
		return
	}
	writeResponse(run(req))
}

func writeResponse(resp Response) {
	enc := json.NewEncoder(os.Stdout)
	if err := enc.Encode(resp); err != nil {
		fmt.Fprintf(os.Stderr, "encode response: %v\n", err)
		os.Exit(1)
	}
}

func run(req Request) Response {
	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, "p.go", req.GoSource, 0)
	if err != nil {
		return Response{FatalError: fmt.Sprintf("parse: %v", err)}
	}

	var typeErrors []string
	conf := types.Config{
		Importer: importer.Default(),
		// Collect but do not abort: a deliberately malformed interface still
		// answers every other question.
		Error: func(err error) { typeErrors = append(typeErrors, err.Error()) },
	}
	pkg, _ := conf.Check("p", fset, []*ast.File{file}, nil)

	results := make([]Answer, 0, len(req.Questions))
	for _, question := range req.Questions {
		switch question.Kind {
		case "selector":
			results = append(results, selectorAnswer(pkg, question))
		case "satisfies":
			results = append(results, satisfiesAnswer(pkg, question))
		default:
			results = append(results, Answer{Kind: question.Kind})
		}
	}
	return Response{TypeErrors: typeErrors, Results: results}
}

func selectorAnswer(pkg *types.Package, question Question) Answer {
	res := Answer{Kind: "selector"}
	root := lookupNamed(pkg, question.Root)
	if root == nil {
		return res
	}
	obj, index, indirect := types.LookupFieldOrMethod(root, true, pkg, question.Member)
	if obj == nil {
		// go/types signals ambiguity by returning a non-nil index path with a
		// nil object; a nil index means simply not found.
		res.Ambiguous = index != nil
		return res
	}
	res.Resolves = true
	// index is the embedded fields traversed plus the final member step, so the
	// embedding depth (0 == declared directly on root) is len(index)-1.
	res.Depth = len(index) - 1
	res.Indirect = indirect
	switch o := obj.(type) {
	case *types.Var:
		res.MemberKind = "field"
		res.ResolvedType = canonical(o.Type())
		res.DeclaringType = fieldDeclaringType(root, index)
	case *types.Func:
		res.MemberKind = "method"
		res.ResolvedType = canonical(o.Type())
		res.DeclaringType = methodDeclaringType(o)
	}
	return res
}

func satisfiesAnswer(pkg *types.Package, question Question) Answer {
	res := Answer{Kind: "satisfies"}
	subject := lookupNamed(pkg, question.TypeName)
	interfaceType := lookupNamed(pkg, question.Interface)
	if subject == nil || interfaceType == nil {
		return res
	}
	underlying, ok := interfaceType.Underlying().(*types.Interface)
	if !ok {
		return res
	}
	res.SatisfiesValue = types.Implements(subject, underlying)
	res.SatisfiesPointer = types.Implements(types.NewPointer(subject), underlying)
	res.Assignable = types.AssignableTo(subject, interfaceType)
	return res
}

func lookupNamed(pkg *types.Package, name string) *types.Named {
	if pkg == nil {
		return nil
	}
	obj := pkg.Scope().Lookup(name)
	if obj == nil {
		return nil
	}
	tn, ok := obj.(*types.TypeName)
	if !ok {
		return nil
	}
	named, _ := tn.Type().(*types.Named)
	return named
}

func methodDeclaringType(fn *types.Func) string {
	signature, ok := fn.Type().(*types.Signature)
	if !ok || signature.Recv() == nil {
		return ""
	}
	return namedName(signature.Recv().Type())
}

// fieldDeclaringType walks the embedding index to the struct that declares the
// field (everything but the last index step navigates embedded fields).
func fieldDeclaringType(root types.Type, index []int) string {
	if len(index) == 0 {
		return ""
	}
	cur := root
	for _, step := range index[:len(index)-1] {
		st := structOf(cur)
		if st == nil || step >= st.NumFields() {
			return ""
		}
		cur = st.Field(step).Type()
	}
	return namedName(cur)
}

func structOf(t types.Type) *types.Struct {
	if ptr, ok := t.(*types.Pointer); ok {
		t = ptr.Elem()
	}
	if named, ok := t.(*types.Named); ok {
		t = named.Underlying()
	}
	st, _ := t.(*types.Struct)
	return st
}

func namedName(t types.Type) string {
	if ptr, ok := t.(*types.Pointer); ok {
		t = ptr.Elem()
	}
	if named, ok := t.(*types.Named); ok {
		return named.Obj().Name()
	}
	return ""
}

func canonical(t types.Type) *CanonicalType {
	switch u := t.(type) {
	case *types.Basic:
		return &CanonicalType{Kind: "basic", Name: basicName(u)}
	case *types.Named:
		return &CanonicalType{Kind: "named", Name: u.Obj().Name()}
	case *types.Pointer:
		return &CanonicalType{Kind: "ref", Element: canonical(u.Elem())}
	case *types.Slice:
		return &CanonicalType{Kind: "slice", Element: canonical(u.Elem())}
	case *types.Signature:
		c := &CanonicalType{Kind: "func"}
		for i := 0; i < u.Params().Len(); i++ {
			c.Parameters = append(c.Parameters, canonical(u.Params().At(i).Type()))
		}
		switch u.Results().Len() {
		case 0:
			c.ReturnType = &CanonicalType{Kind: "basic", Name: "unit"}
		case 1:
			c.ReturnType = canonical(u.Results().At(0).Type())
		default:
			c.ReturnType = &CanonicalType{Kind: "other", Name: "tuple"}
		}
		return c
	default:
		return &CanonicalType{Kind: "other", Name: t.String()}
	}
}

func basicName(b *types.Basic) string {
	switch b.Kind() {
	case types.Int:
		return "int"
	case types.Float64:
		return "float"
	case types.String:
		return "string"
	case types.Bool:
		return "bool"
	case types.Uint8: // byte
		return "byte"
	case types.Int32: // rune
		return "rune"
	default:
		return b.Name()
	}
}
