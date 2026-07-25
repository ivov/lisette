package convert

import (
	"go/types"
	"math/bits"
	"slices"
	"strconv"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/extract"
)

type constInfo struct {
	object types.Object
	result ConvertResult
}

type convertedSymbol struct {
	export extract.SymbolExport
	result ConvertResult
}

func classifyConstGroups(symbols []convertedSymbol, cfg *config.Config, pkgPath string) (regular []ConvertResult, groups []ConstGroup, bitFlagSetTypeNames map[string]bool) {
	typeToConstants := make(map[string][]constInfo)
	typeToUnderlying := make(map[string]string)
	typeResults := make(map[string]ConvertResult)

	for _, symbol := range symbols {
		result := symbol.result
		if result.Kind == extract.ExportType {
			typeResults[result.Name] = result
		}
		if result.Kind != extract.ExportConstant {
			continue
		}
		if result.SkipReason != nil {
			continue
		}
		if result.ConstValue == "" {
			continue
		}

		constObj, ok := symbol.export.Obj.(*types.Const)
		if !ok {
			continue
		}

		namedType, ok := constObj.Type().(*types.Named)
		if !ok {
			continue
		}

		typeObj := namedType.Obj()
		if typeObj.Pkg() == nil || typeObj.Pkg() != constObj.Pkg() {
			continue
		}

		// Unexported types must not leak as Lisette type declarations;
		// their typed constants leak as untyped `pub const X = N`.
		if !typeObj.Exported() {
			continue
		}

		underlying := namedType.Underlying()
		basic, ok := underlying.(*types.Basic)
		if !ok {
			continue
		}

		if basic.Info()&types.IsInteger == 0 && basic.Info()&types.IsString == 0 {
			continue
		}

		typeName := typeObj.Name()

		if _, exists := typeToUnderlying[typeName]; !exists {
			typeToUnderlying[typeName] = basic.Name()
		}

		typeToConstants[typeName] = append(typeToConstants[typeName], constInfo{
			object: symbol.export.Obj,
			result: result,
		})
	}

	bitFlagSetTypeNames = make(map[string]bool)
	groupedObjects := make(map[types.Object]bool)
	groupedTypeNames := make(map[string]bool)

	typeNames := make([]string, 0, len(typeToConstants))
	for typeName := range typeToConstants {
		typeNames = append(typeNames, typeName)
	}
	slices.Sort(typeNames)

	for _, typeName := range typeNames {
		constants := typeToConstants[typeName]
		if len(constants) < 2 {
			continue
		}
		typeResult, hasType := typeResults[typeName]
		if !hasType {
			continue
		}

		// Bit operations on a string-underlying type are not meaningful;
		// neither H13 nor the config override applies here.
		isInteger := typeToUnderlying[typeName] != "string"
		if isInteger && !cfg.IsClosedDomain(pkgPath, typeName) &&
			(cfg.ShouldTreatAsBitFlagSet(pkgPath, typeName) || looksLikeBitFlags(constants)) {
			bitFlagSetTypeNames[typeName] = true
			continue
		}

		constantResults := make([]ConvertResult, 0, len(constants))
		for _, constant := range constants {
			constantResults = append(constantResults, constant.result)
			groupedObjects[constant.object] = true
		}
		groups = append(groups, ConstGroup{
			Type:      typeResult,
			Constants: constantResults,
		})
		groupedTypeNames[typeName] = true
	}

	regular = make([]ConvertResult, 0, len(symbols))
	for _, symbol := range symbols {
		if groupedObjects[symbol.export.Obj] ||
			(symbol.result.Kind == extract.ExportType && groupedTypeNames[symbol.result.Name]) {
			continue
		}
		regular = append(regular, symbol.result)
	}
	return regular, groups, bitFlagSetTypeNames
}

// looksLikeBitFlags classifies a named integer type as a bit-flag set.
// Rule (H13): at least 4 constants, every nonzero value is a single bit,
// and the values are not the sequential range 0..N-1 or 1..N. Small flag
// types (under 4 constants) and hybrid mask/flag types pass through as plain
// const groups; recover them via the bit_flag_set config override.
func looksLikeBitFlags(constants []constInfo) bool {
	const minConstants = 4
	if len(constants) < minConstants {
		return false
	}

	if isSequentialRange(constants) {
		return false
	}

	for _, c := range constants {
		val := parseIntValue(c.result.ConstValue)
		if val == 0 {
			continue
		}
		if val < 0 || bits.OnesCount64(uint64(val)) != 1 {
			return false
		}
	}
	return true
}

// isSequentialRange reports whether the constant values form 0..N-1 or 1..N.
func isSequentialRange(constants []constInfo) bool {
	vals := make([]int64, 0, len(constants))
	for _, c := range constants {
		vals = append(vals, parseIntValue(c.result.ConstValue))
	}
	slices.Sort(vals)
	if vals[0] != 0 && vals[0] != 1 {
		return false
	}
	for i := 1; i < len(vals); i++ {
		if vals[i] != vals[i-1]+1 {
			return false
		}
	}
	return true
}

func parseIntValue(s string) int64 {
	negative := strings.HasPrefix(s, "-")
	s = strings.TrimPrefix(s, "-")

	var val int64
	var err error

	switch {
	case strings.HasPrefix(s, "0x") || strings.HasPrefix(s, "0X"):
		val, err = strconv.ParseInt(s[2:], 16, 64)
	case strings.HasPrefix(s, "0o") || strings.HasPrefix(s, "0O"):
		val, err = strconv.ParseInt(s[2:], 8, 64)
	case strings.HasPrefix(s, "0b") || strings.HasPrefix(s, "0B"):
		val, err = strconv.ParseInt(s[2:], 2, 64)
	default:
		val, err = strconv.ParseInt(s, 10, 64)
	}

	if err != nil {
		return 0
	}

	if negative {
		return -val
	}
	return val
}
