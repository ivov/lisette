package convert

import (
	"go/constant"
	"go/token"
	"go/types"
	"testing"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

func mkConsts(values ...string) []constInfo {
	out := make([]constInfo, len(values))
	for i, v := range values {
		out[i] = constInfo{result: ConvertResult{ConstValue: v}}
	}
	return out
}

func TestLooksLikeBitFlags_SmallSequential_AreConstGroups(t *testing.T) {
	cases := []struct {
		name string
		vals []string
	}{
		{"{0,1}", []string{"0", "1"}},
		{"{1,2}", []string{"1", "2"}},
		{"{0,1,2}", []string{"0", "1", "2"}},
		{"{1,2,3}", []string{"1", "2", "3"}},
		{"{0,1,2,3,4}", []string{"0", "1", "2", "3", "4"}},
		{"{1,2,3,4}", []string{"1", "2", "3", "4"}},
		{"{1,2,3,4,5}", []string{"1", "2", "3", "4", "5"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if looksLikeBitFlags(mkConsts(c.vals...)) {
				t.Errorf("%s: want const group, got flags", c.name)
			}
		})
	}
}

func TestLooksLikeBitFlags_TextbookFlags(t *testing.T) {
	cases := []struct {
		name string
		vals []string
	}{
		{"{1,2,4,8}", []string{"1", "2", "4", "8"}},
		{"{0,1,2,4,8}", []string{"0", "1", "2", "4", "8"}},
		{"{1,2,4,8,16}", []string{"1", "2", "4", "8", "16"}},
		{"{1,2,4,8,16,32}", []string{"1", "2", "4", "8", "16", "32"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if !looksLikeBitFlags(mkConsts(c.vals...)) {
				t.Errorf("%s: want flags, got const group", c.name)
			}
		})
	}
}

func TestLooksLikeBitFlags_BelowMinConstants(t *testing.T) {
	if looksLikeBitFlags(mkConsts("1", "2", "4")) {
		t.Error("3-value {1,2,4}: want const group (recover via config), got flags")
	}
}

func TestLooksLikeBitFlags_HybridMaskAndBits(t *testing.T) {
	if looksLikeBitFlags(mkConsts("1", "2", "4", "8", "0xff00000")) {
		t.Error("hybrid mask+bits: want const group (recover via config), got flags")
	}
}

func TestIsSequentialRange(t *testing.T) {
	cases := []struct {
		name string
		vals []string
		want bool
	}{
		{"{0,1,2,3,4}", []string{"0", "1", "2", "3", "4"}, true},
		{"{1,2,3,4,5}", []string{"1", "2", "3", "4", "5"}, true},
		{"reversed-order {4,3,2,1,0}", []string{"4", "3", "2", "1", "0"}, true},
		{"{0,1,2,4}", []string{"0", "1", "2", "4"}, false},
		{"{1,2,4,8}", []string{"1", "2", "4", "8"}, false},
		{"starts-at-2 {2,3,4}", []string{"2", "3", "4"}, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := isSequentialRange(mkConsts(c.vals...))
			if got != c.want {
				t.Errorf("%s: want %v, got %v", c.name, c.want, got)
			}
		})
	}
}

func TestClassifyConstGroupsMovesTypeAndConstantsOutOfRegularSymbols(t *testing.T) {
	pkg := types.NewPackage("example.com/status", "status")
	typeObj := types.NewTypeName(token.NoPos, pkg, "Status", nil)
	named := types.NewNamed(typeObj, types.Typ[types.Int], nil)
	first := types.NewConst(token.NoPos, pkg, "Ready", named, constant.MakeInt64(1))
	second := types.NewConst(token.NoPos, pkg, "Done", named, constant.MakeInt64(2))
	symbols := []convertedSymbol{
		{export: extract.SymbolExport{Obj: typeObj}, result: ConvertResult{Name: "Status", Kind: extract.ExportType}},
		{export: extract.SymbolExport{Obj: first}, result: ConvertResult{Name: "Ready", Kind: extract.ExportConstant, ConstValue: "1"}},
		{export: extract.SymbolExport{Obj: second}, result: ConvertResult{Name: "Done", Kind: extract.ExportConstant, ConstValue: "2"}},
	}

	regular, groups, _ := classifyConstGroups(symbols, nil, pkg.Path())

	if len(regular) != 0 || len(groups) != 1 || groups[0].Type.Name != "Status" || len(groups[0].Constants) != 2 {
		t.Fatalf("classification = regular %#v, groups %#v", regular, groups)
	}
}
