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

// ConstGroupInfo describes a Go defined primitive type (e.g. `type Duration
// int64`) together with the constants declared at that type. It emits as a named
// primitive struct plus package-level typed constants.
type ConstGroupInfo struct {
	TypeName       string
	UnderlyingType string // e.g., "int64" for time.Duration
	Variants       []EnumVariant
}

type constInfo struct {
	index int
	name  string
	value string
}

// DetectConstGroups groups exported constants by their named primitive type,
// returning one ConstGroupInfo per group (minimum two constants, not a bit-flag
// set). `constGroupTypeNames` is the set of type names that own such a group.
func DetectConstGroups(results []ConvertResult, exports []extract.SymbolExport, cfg *config.Config, pkgPath string) (constGroups []ConstGroupInfo, constantTypes map[int]string, constGroupTypeNames map[string]bool, bitFlagSetTypeNames map[string]bool) {
	typeToConstants := make(map[string][]constInfo)
	typeToUnderlying := make(map[string]string)

	for i, result := range results {
		if result.Kind != extract.ExportConstant {
			continue
		}
		if result.SkipReason != nil {
			continue
		}
		if result.ConstValue == "" {
			continue
		}

		exp := exports[i]
		constObj, ok := exp.Obj.(*types.Const)
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
			index: i,
			name:  result.Name,
			value: result.ConstValue,
		})
	}

	constantTypes = make(map[int]string)
	constGroupTypeNames = make(map[string]bool)
	bitFlagSetTypeNames = make(map[string]bool)

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

		// Bit operations on a string-underlying type are not meaningful;
		// neither H13 nor the config override applies here.
		isInteger := typeToUnderlying[typeName] != "string"
		if isInteger && !cfg.IsClosedDomain(pkgPath, typeName) &&
			(cfg.ShouldTreatAsBitFlagSet(pkgPath, typeName) || looksLikeBitFlags(constants)) {
			bitFlagSetTypeNames[typeName] = true
			continue
		}

		var variants []EnumVariant
		for _, c := range constants {
			variants = append(variants, EnumVariant{
				Name:  c.name,
				Value: c.value,
			})
			constantTypes[c.index] = typeName
		}

		constGroups = append(constGroups, ConstGroupInfo{
			TypeName:       typeName,
			UnderlyingType: typeToUnderlying[typeName],
			Variants:       variants,
		})
		constGroupTypeNames[typeName] = true
	}

	return constGroups, constantTypes, constGroupTypeNames, bitFlagSetTypeNames
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
		val := parseIntValue(c.value)
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
		vals = append(vals, parseIntValue(c.value))
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
