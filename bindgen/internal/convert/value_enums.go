package convert

import (
	"go/types"
	"math/bits"
	"slices"
	"strconv"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

type ValueEnumInfo struct {
	TypeName       string
	UnderlyingType string // e.g., "int64" for time.Duration
	Variants       []EnumVariant
}

func DetectValueEnums(results []ConvertResult, exports []extract.SymbolExport) ([]ValueEnumInfo, map[int]string, map[string]bool) {
	typeToConstants := make(map[string][]struct {
		index int
		name  string
		value string
	})
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

		underlying := namedType.Underlying()
		basic, ok := underlying.(*types.Basic)
		if !ok {
			continue
		}

		if basic.Info()&types.IsInteger == 0 && basic.Info()&types.IsString == 0 {
			continue
		}

		typeName := namedType.Obj().Name()

		if _, exists := typeToUnderlying[typeName]; !exists {
			typeToUnderlying[typeName] = basic.Name()
		}

		typeToConstants[typeName] = append(typeToConstants[typeName], struct {
			index int
			name  string
			value string
		}{
			index: i,
			name:  result.Name,
			value: result.ConstValue,
		})
	}

	var valueEnums []ValueEnumInfo
	constantTypes := make(map[int]string)
	valueEnumTypeNames := make(map[string]bool)

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

		if looksLikeBitFlags(constants) {
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

		valueEnums = append(valueEnums, ValueEnumInfo{
			TypeName:       typeName,
			UnderlyingType: typeToUnderlying[typeName],
			Variants:       variants,
		})
		valueEnumTypeNames[typeName] = true
	}

	return valueEnums, constantTypes, valueEnumTypeNames
}

func looksLikeBitFlags(constants []struct {
	index int
	name  string
	value string
}) bool {
	if len(constants) < 2 {
		return false
	}

	powersOf2 := 0
	for _, c := range constants {
		val := parseIntValue(c.value)
		if val > 0 && bits.OnesCount64(uint64(val)) == 1 {
			powersOf2++
		}
	}

	return powersOf2 > len(constants)/2
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
