package convert

import (
	"go/types"
	"testing"
)

func signatureOf(t *testing.T, scope *types.Scope, name string) *types.Signature {
	t.Helper()
	fn, _ := scope.Lookup(name).(*types.Func)
	if fn == nil {
		t.Fatalf("no function %q", name)
	}
	return fn.Type().(*types.Signature)
}

func methodSignatureOf(t *testing.T, scope *types.Scope, typeName, methodName string) *types.Signature {
	t.Helper()
	obj := scope.Lookup(typeName)
	if obj == nil {
		t.Fatalf("no type %q", typeName)
	}
	iface, ok := obj.Type().Underlying().(*types.Interface)
	if !ok {
		t.Fatalf("%q is not an interface", typeName)
	}
	for method := range iface.Methods() {
		if method.Name() == methodName {
			return method.Type().(*types.Signature)
		}
	}
	t.Fatalf("no method %q on %q", methodName, typeName)
	return nil
}

func TestParamNamesSpellEveryParameter(t *testing.T) {
	_, pkg := analyzeSource(t, `
type Request struct{ Path string }

type Handler interface {
	Serve(*Request, []byte)
	Merge([]byte, []byte)
}

func Fill(self []int, s string) {}

func Take(map[string]int) {}
`)
	scope := pkg.Types.Scope()

	cases := []struct {
		name string
		sig  *types.Signature
		want []string
	}{
		{"unnamed pointer and slice", methodSignatureOf(t, scope, "Handler", "Serve"), []string{"request", "b"}},
		{"unnamed duplicates", methodSignatureOf(t, scope, "Handler", "Merge"), []string{"b", "b2"}},
		{"reserved keyword", signatureOf(t, scope, "Fill"), []string{"self_", "s"}},
		{"unnamed map", signatureOf(t, scope, "Take"), []string{"m"}},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := paramNames(c.sig)
			if len(got) != len(c.want) {
				t.Fatalf("paramNames = %v, want %v", got, c.want)
			}
			for i := range c.want {
				if got[i] != c.want[i] {
					t.Fatalf("paramNames = %v, want %v", got, c.want)
				}
			}
		})
	}
}
