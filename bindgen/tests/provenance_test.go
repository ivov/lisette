package tests

import (
	"fmt"
	"go/types"
	"os"
	"slices"
	"sort"
	"strings"
	"testing"

	"github.com/ivov/lisette/bindgen/internal/cli"
	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/convert"
	"github.com/ivov/lisette/bindgen/internal/extract"
)

// stdViews holds the resolved view fact per bound function, by signature
// result index: "s" for whole-value sharing, "[s]" for element-level, "recv"
// for the receiver.
type stdViews struct {
	tokens     map[string]map[int][]string
	unanalyzed map[string]bool
	opaque     map[string][]int
	shared     map[string][]int
}

// TestStdReturnViews checks the derivation against the hand-audited
// checklist and a golden file, so Go version bumps surface as diffs. The
// strings subtest exempts the shared marker, which states io.EOF sentinels
// outside the audited no-aliasing claim.
func TestStdReturnViews(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stdlib view derivation in short mode")
	}

	views := deriveStdViews(t)

	t.Run("Checklist", func(t *testing.T) { checkViewChecklist(t, views) })
	t.Run("StringsStaysFresh", func(t *testing.T) {
		for name, perResult := range views.tokens {
			if !strings.HasPrefix(name, "strings.") {
				continue
			}
			if len(perResult) > 0 {
				t.Errorf("%s reports a view, strings results are fresh or immutable", name)
			}
			if len(views.opaque[name]) > 0 {
				t.Errorf("%s went opaque, strings results are fresh or immutable", name)
			}
		}
	})
	t.Run("Golden", func(t *testing.T) { checkViewGolden(t, views) })
}

// deriveStdViews loads linux/amd64, the canonical generation target.
func deriveStdViews(t *testing.T) stdViews {
	t.Helper()
	pkgs, err := extract.LoadStdPackages("linux", "amd64", cli.SkipStdPackage)
	if err != nil {
		t.Fatalf("failed to load std packages: %v", err)
	}
	cfg, err := config.LoadConfig("../bindgen.stdlib.json", nil)
	if err != nil {
		t.Fatalf("failed to load stdlib config: %v", err)
	}
	nilness := convert.NewNilnessAnalysis(pkgs, &cfg)
	if nilness == nil {
		t.Fatal("SSA build failed")
	}
	mutation := convert.NewMutationAnalysis(nilness, pkgs)
	if mutation == nil {
		t.Fatal("mutation analysis unavailable")
	}

	views := stdViews{
		tokens:     map[string]map[int][]string{},
		unanalyzed: map[string]bool{},
		opaque:     map[string][]int{},
		shared:     map[string][]int{},
	}
	record := func(pkgPath string, fn *types.Func) {
		derived, ok := mutation.Views(fn)
		if !ok {
			return
		}
		sig := fn.Type().(*types.Signature)
		lookupName := receiverQualifiedName(fn)
		overrides, hasOverride := cfg.ViewOverrides(pkgPath, lookupName)
		resolved, invalidOverrides := convert.ResolveViews(derived, overrides, hasOverride, sig)
		if len(invalidOverrides) > 0 {
			t.Errorf("%s.%s: invalid returns_view_of entries %v", pkgPath, lookupName, invalidOverrides)
		}
		if resolved.Fresh() {
			return
		}
		name := pkgPath + "." + lookupName
		perResult := map[int][]string{}
		var opaqueResults, sharedResults []int
		for i, result := range resolved.Results {
			if result.Opaque {
				opaqueResults = append(opaqueResults, i)
			}
			if result.Shared {
				sharedResults = append(sharedResults, i)
			}
			tokens := aliasTokens(sig, result)
			if len(tokens) > 0 {
				perResult[i] = tokens
			}
		}
		views.tokens[name] = perResult
		if !resolved.Analyzed {
			views.unanalyzed[name] = true
		}
		if len(opaqueResults) > 0 {
			views.opaque[name] = opaqueResults
		}
		if len(sharedResults) > 0 {
			views.shared[name] = sharedResults
		}
	}

	for _, pkg := range pkgs {
		scope := pkg.Types.Scope()
		for _, scopeName := range scope.Names() {
			switch obj := scope.Lookup(scopeName).(type) {
			case *types.Func:
				if obj.Exported() {
					record(pkg.PkgPath, obj)
				}
			case *types.TypeName:
				if !obj.Exported() {
					continue
				}
				named, ok := obj.Type().(*types.Named)
				if !ok {
					continue
				}
				methodSet := types.NewMethodSet(types.NewPointer(named))
				for i := 0; i < methodSet.Len(); i++ {
					method, ok := methodSet.At(i).Obj().(*types.Func)
					if ok && method.Exported() && method.Pkg() == pkg.Types {
						record(pkg.PkgPath, method)
					}
				}
			}
		}
	}
	return views
}

// receiverQualifiedName matches the config key format.
func receiverQualifiedName(fn *types.Func) string {
	sig := fn.Type().(*types.Signature)
	receiver := sig.Recv()
	if receiver == nil {
		return fn.Name()
	}
	receiverType := receiver.Type()
	if pointer, ok := receiverType.(*types.Pointer); ok {
		receiverType = pointer.Elem()
	}
	if named, ok := receiverType.(*types.Named); ok {
		return named.Obj().Name() + "." + fn.Name()
	}
	return fn.Name()
}

func aliasTokens(sig *types.Signature, result convert.ResultView) []string {
	var out []string
	switch result.Receiver {
	case convert.DepthWhole:
		out = append(out, "recv")
	case convert.DepthElement:
		out = append(out, "[recv]")
	}
	for i := 0; i < sig.Params().Len(); i++ {
		name := sig.Params().At(i).Name()
		if name == "" {
			name = fmt.Sprintf("p%d", i)
		}
		switch result.Param(i) {
		case convert.DepthWhole:
			out = append(out, name)
		case convert.DepthElement:
			out = append(out, "["+name+"]")
		}
	}
	sort.Strings(out)
	return out
}

// viewChecklist pins each audited result's exact aliasing set: every listed
// token must derive, and anything beyond it and conservativeViewTokens fails.
var viewChecklist = []struct {
	name   string
	result int
	want   []string
}{
	{"slices.Clip", 0, []string{"s"}},
	{"slices.Compact", 0, []string{"s"}},
	{"slices.CompactFunc", 0, []string{"s"}},
	{"slices.Delete", 0, []string{"s"}},
	{"slices.DeleteFunc", 0, []string{"s"}},
	{"slices.Grow", 0, []string{"s"}},
	{"slices.Insert", 0, []string{"s", "[v]"}},
	{"slices.Replace", 0, []string{"s", "[v]"}},
	{"slices.AppendSeq", 0, []string{"s"}},

	{"bytes.Cut", 0, []string{"s"}},
	{"bytes.Cut", 1, []string{"s"}},
	{"bytes.CutPrefix", 0, []string{"s"}},
	{"bytes.CutSuffix", 0, []string{"s"}},
	{"bytes.Trim", 0, []string{"s"}},
	{"bytes.TrimFunc", 0, []string{"s"}},
	{"bytes.TrimLeft", 0, []string{"s"}},
	{"bytes.TrimLeftFunc", 0, []string{"s"}},
	{"bytes.TrimRight", 0, []string{"s"}},
	{"bytes.TrimRightFunc", 0, []string{"s"}},
	{"bytes.TrimPrefix", 0, []string{"s"}},
	{"bytes.TrimSuffix", 0, []string{"s"}},
	{"bytes.TrimSpace", 0, []string{"s"}},
	{"bytes.Fields", 0, []string{"[s]"}},
	{"bytes.FieldsFunc", 0, []string{"[s]"}},
	{"bytes.Split", 0, []string{"[s]"}},
	{"bytes.SplitAfter", 0, []string{"[s]"}},
	{"bytes.SplitAfterN", 0, []string{"[s]"}},
	{"bytes.SplitN", 0, []string{"[s]"}},
	{"bytes.SplitSeq", 0, []string{"[s]"}},
	{"bytes.SplitAfterSeq", 0, []string{"[s]"}},
	{"bytes.FieldsSeq", 0, []string{"[s]"}},
	{"bytes.FieldsFuncSeq", 0, []string{"[s]"}},
	{"bytes.Lines", 0, []string{"[s]"}},
	{"bytes.Buffer.Bytes", 0, []string{"recv"}},
	{"bytes.Buffer.AvailableBuffer", 0, []string{"recv"}},
	{"bytes.Buffer.Next", 0, []string{"recv"}},
	{"bytes.NewBuffer", 0, []string{"[buf]"}},
	{"bytes.NewReader", 0, []string{"[b]"}},

	{"bufio.ScanBytes", 1, []string{"data"}},
	{"bufio.ScanLines", 1, []string{"data"}},
	// ScanRunes also returns the errorRune global, stated by the shared marker
	{"bufio.ScanRunes", 1, []string{"data"}},
	{"bufio.ScanWords", 1, []string{"data"}},
	{"bufio.Reader.Peek", 0, []string{"recv"}},
	{"bufio.Reader.ReadSlice", 0, []string{"recv"}},
	{"bufio.Reader.ReadLine", 0, []string{"recv"}},
	{"bufio.Writer.AvailableBuffer", 0, []string{"recv"}},
	{"bufio.Scanner.Bytes", 0, []string{"recv"}},

	{"unicode/utf8.AppendRune", 0, []string{"p"}},

	{"strconv.AppendBool", 0, []string{"dst"}},
	{"strconv.AppendFloat", 0, []string{"dst"}},
	{"strconv.AppendInt", 0, []string{"dst"}},
	{"strconv.AppendUint", 0, []string{"dst"}},
	{"strconv.AppendQuote", 0, []string{"dst"}},
	{"strconv.AppendQuoteRune", 0, []string{"dst"}},
	{"strconv.AppendQuoteRuneToASCII", 0, []string{"dst"}},
	{"strconv.AppendQuoteRuneToGraphic", 0, []string{"dst"}},
	{"strconv.AppendQuoteToASCII", 0, []string{"dst"}},
	{"strconv.AppendQuoteToGraphic", 0, []string{"dst"}},

	{"encoding/hex.AppendEncode", 0, []string{"dst"}},
	{"encoding/hex.AppendDecode", 0, []string{"dst"}},
	{"encoding/base32.Encoding.AppendEncode", 0, []string{"dst"}},
	{"encoding/base32.Encoding.AppendDecode", 0, []string{"dst"}},
	{"encoding/base64.Encoding.AppendEncode", 0, []string{"dst"}},
	{"encoding/base64.Encoding.AppendDecode", 0, []string{"dst"}},
	{"encoding/binary.Append", 0, []string{"buf"}},
	{"encoding/binary.AppendUvarint", 0, []string{"buf"}},
	{"encoding/binary.AppendVarint", 0, []string{"buf"}},

	{"math/big.Float.Append", 0, []string{"buf"}},
	{"math/big.Int.Append", 0, []string{"buf"}},
	{"math/big.Int.FillBytes", 0, []string{"buf"}},

	{"net/textproto.TrimBytes", 0, []string{"b"}},
	{"net/textproto.MIMEHeader.Values", 0, []string{"recv"}},
	{"net/http.Header.Values", 0, []string{"recv"}},
}

// conservativeViewTokens documents accepted over-approximation: iterator
// captures are marked wholesale, so the split iterators report [sep] although
// yielded slices view only s. Stopping deriving these keeps passing.
var conservativeViewTokens = map[string]map[int][]string{
	"bytes.SplitSeq":      {0: {"[sep]"}},
	"bytes.SplitAfterSeq": {0: {"[sep]"}},
}

func checkViewChecklist(t *testing.T, views stdViews) {
	for _, entry := range viewChecklist {
		perResult, ok := views.tokens[entry.name]
		if !ok {
			t.Errorf("%s: no view derived, checklist expects result %d to alias %v",
				entry.name, entry.result, entry.want)
			continue
		}
		if views.unanalyzed[entry.name] {
			t.Errorf("%s: fell back to the worst case, checklist expects a derived fact", entry.name)
			continue
		}
		if slices.Contains(views.opaque[entry.name], entry.result) {
			t.Errorf("%s: result %d went opaque, checklist expects a precise fact",
				entry.name, entry.result)
			continue
		}
		derived := perResult[entry.result]
		accepted := conservativeViewTokens[entry.name][entry.result]
		for _, want := range entry.want {
			if !slices.Contains(derived, want) {
				t.Errorf("%s: result %d aliases %v, checklist expects %s",
					entry.name, entry.result, derived, want)
			}
		}
		for _, token := range derived {
			if !slices.Contains(entry.want, token) && !slices.Contains(accepted, token) {
				t.Errorf("%s: result %d derives %s beyond the audited set %v",
					entry.name, entry.result, token, entry.want)
			}
		}
	}
}

func checkViewGolden(t *testing.T, views stdViews) {
	renderIndexes := func(label string, indexes []int) string {
		if len(indexes) == 0 {
			return ""
		}
		rendered := make([]string, len(indexes))
		for i, index := range indexes {
			rendered[i] = fmt.Sprintf("r%d", index)
		}
		return fmt.Sprintf(" (%s %s)", label, strings.Join(rendered, " "))
	}

	var lines []string
	for name, perResult := range views.tokens {
		marker := ""
		if views.unanalyzed[name] {
			marker = " (unanalyzed)"
		} else {
			marker = renderIndexes("opaque", views.opaque[name]) +
				renderIndexes("shared", views.shared[name])
		}
		var rendered []string
		indexes := make([]int, 0, len(perResult))
		for index := range perResult {
			indexes = append(indexes, index)
		}
		sort.Ints(indexes)
		for _, index := range indexes {
			for _, token := range perResult[index] {
				rendered = append(rendered, fmt.Sprintf("r%d<-%s", index, token))
			}
		}
		lines = append(lines, fmt.Sprintf("%s%s: %s", name, marker, strings.Join(rendered, " ")))
	}
	sort.Strings(lines)
	output := strings.Join(lines, "\n") + "\n"

	goldenPath := "testdata/provenance_std.golden"
	if *update {
		if err := os.WriteFile(goldenPath, []byte(output), 0644); err != nil {
			t.Fatalf("failed to write golden: %v", err)
		}
		return
	}
	expected, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("golden not found: %s (run with -update to create)", goldenPath)
	}
	if diff := diffOutput(expected, []byte(output)); diff != "" {
		t.Errorf("derived views changed:\n%s", diff)
	}
}
