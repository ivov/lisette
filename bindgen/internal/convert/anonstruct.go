package convert

import (
	"fmt"
	"go/types"
	"sort"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/extract"
)

const anonStructPrefix = "Anon"

func anonStructSkip() *SkipReason {
	return &SkipReason{
		Code:    "anonymous-struct",
		Message: "anonymous struct types are not supported",
	}
}

type syntheticStruct struct {
	key    string // shape identity, for de-duplicating identical shapes
	result ConvertResult
}

// internAnonStruct mints (or reuses, by shape) a named Lisette struct standing
// in for a Go anonymous struct. It skips on a tag, unexported field, or
// unrepresentable field type: each would make the stand-in's underlying type
// differ from the Go original, so the emitted Go would not be assignable.
func (c *Converter) internAnonStruct(s *types.Struct) TypeResult {
	for i := 0; i < s.NumFields(); i++ {
		if !s.Field(i).Exported() || s.Tag(i) != "" {
			return TypeResult{SkipReason: anonStructSkip()}
		}
	}

	checkpoint := len(c.synth)
	fields, skip := c.convertAnonFields(s)
	if skip != nil {
		c.rollbackSynth(checkpoint)
		return TypeResult{SkipReason: skip}
	}

	key := anonShapeKey(fields)
	if idx, ok := c.synthByShape[key]; ok {
		return TypeResult{LisetteType: c.synth[idx].result.Name}
	}

	name := c.mintAnonName(fields)
	c.synthByShape[key] = len(c.synth)
	c.synthTaken[name] = true
	c.synth = append(c.synth, syntheticStruct{
		key: key,
		result: ConvertResult{
			Name:       name,
			Kind:       extract.ExportType,
			Doc:        "Synthesized from a Go anonymous struct type.",
			Fields:     fields,
			AnonStruct: true,
		},
	})
	return TypeResult{LisetteType: name}
}

// convertAnonFields fails the whole struct if any field is unrepresentable.
// Unlike convertStructFields, which drops bad fields and keeps the named
// struct, an anonymous struct must reproduce its Go underlying type exactly.
func (c *Converter) convertAnonFields(s *types.Struct) ([]StructField, *SkipReason) {
	fields := make([]StructField, 0, s.NumFields())
	for i := 0; i < s.NumFields(); i++ {
		field := s.Field(i)
		fieldType := ToLisetteNilable(field.Type(), c)
		if fieldType.SkipReason != nil {
			return nil, fieldType.SkipReason
		}
		fields = append(fields, StructField{Name: field.Name(), Type: fieldType.LisetteType})
	}
	return fields, nil
}

// anonShapeKey is injective because NUL appears in neither field names nor types.
func anonShapeKey(fields []StructField) string {
	var b strings.Builder
	for _, f := range fields {
		b.WriteString(f.Name)
		b.WriteByte(0)
		b.WriteString(f.Type)
		b.WriteByte(0)
	}
	return b.String()
}

// mintAnonName builds a display name from the field names, uniquified against
// synthesized and package-declared names.
func (c *Converter) mintAnonName(fields []StructField) string {
	var b strings.Builder
	b.WriteString(anonStructPrefix)
	for _, f := range fields {
		b.WriteString(f.Name)
	}
	base := b.String()

	c.seedReservedNames()
	name := base
	for i := 2; c.synthTaken[name]; i++ {
		name = fmt.Sprintf("%s_%d", base, i)
	}
	return name
}

// seedReservedNames reserves the package's declared names so a synthesized name
// never clashes with a real type. Runs once.
func (c *Converter) seedReservedNames() {
	if c.reservedSeeded {
		return
	}
	c.reservedSeeded = true
	if c.pkg == nil || c.pkg.Types == nil {
		return
	}
	for _, name := range c.pkg.Types.Scope().Names() {
		c.synthTaken[name] = true
	}
}

// synthMark/rollbackSynth bracket a probe whose result may be discarded, so any
// type minted during it does not leak into the output as an orphan.
func (c *Converter) synthMark() int {
	if c == nil {
		return 0
	}
	return len(c.synth)
}

func (c *Converter) rollbackSynth(checkpoint int) {
	if c == nil {
		return
	}
	for i := checkpoint; i < len(c.synth); i++ {
		delete(c.synthByShape, c.synth[i].key)
		delete(c.synthTaken, c.synth[i].result.Name)
	}
	c.synth = c.synth[:checkpoint]
}

// SyntheticStructs returns the synthesized types, name-sorted for stable emission.
func (c *Converter) SyntheticStructs() []ConvertResult {
	out := make([]ConvertResult, len(c.synth))
	for i, s := range c.synth {
		out[i] = s.result
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out
}
