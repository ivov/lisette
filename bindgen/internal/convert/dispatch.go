package convert

import (
	"go/ast"
	"go/token"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/tools/go/packages"
)

type ConvertResult struct {
	Name             string
	Kind             extract.SymbolExportKind
	Doc              string
	LisetteType      string
	Params           []FunctionParameter
	ReturnType       string
	Receiver         *Receiver // for methods
	TypeParams       TypeParamSpecs
	Fields           []StructField     // for structs
	InterfaceMethods []InterfaceMethod // for interfaces
	Variants         []EnumVariant     // for enums (via iota)
	ConstValue       string            // for constants
	SkipReason       *SkipReason
	IsInterface      bool // true when this type should be emitted as `pub interface`
	IsTypeAlias      bool // true for Go type aliases (type X = Y)
	CommaOk          bool // true when return is from (T, bool) comma-ok with nilable T
	ArrayReturn      bool // true when Go type is [N]T but Lisette type is Slice<T>
	// SentinelInt is set when this function returns int but the bindgen
	// config declares a magic value (e.g. -1) for "not found". Bindgen
	// rewrites the return type to Option<int> and emits the matching
	// flag-name annotation (e.g. `#[go(sentinel_minus_one)]`).
	SentinelInt *int
	// BuilderMethod suppresses unused_value on fluent-chain returns the caller typically discards.
	BuilderMethod bool
}

type FunctionParameter struct {
	Name    string
	Type    string
	Mutable bool
}

type Receiver struct {
	Name         string
	Type         string
	IsPointer    bool
	BaseTypeName string
	TypeParams   TypeParamSpecs // Type parameters of the receiver type (for generic types)
}

type StructField struct {
	Name string
	Type string
	Doc  string
}

type InterfaceMethod struct {
	Name        string
	Params      []FunctionParameter
	ReturnType  string
	CommaOk     bool
	ArrayReturn bool
}

type EnumVariant struct {
	Name  string
	Value string
}

// ExternalPkgs maps package paths to package names (e.g., "time" -> "time").
type ExternalPkgs map[string]string

// ASCII SOH/STX, used to wrap a package path in reference strings so the
// emitter can substitute it with the resolved local prefix after collision
// detection. Neither byte can appear in identifiers or doc text.
const (
	PkgRefStart = "\x01"
	PkgRefEnd   = "\x02"
)

func PkgRef(path string) string {
	return PkgRefStart + path + PkgRefEnd
}

type Converter struct {
	currentPkgPath       string
	externalPkgs         ExternalPkgs
	pkg                  *packages.Package
	cfg                  *config.Config
	uniformPointerTypes  map[string]bool              // lazily computed; types with 10+ single-pointer-return methods
	manyToOneTypes       map[string]bool              // lazily computed; return types with 10+ free functions
	majorityPointerTypes map[string]bool              // lazily computed; types where ≥20 methods return same *T (>90%)
	funcDeclCache        map[token.Pos]*ast.FuncDecl  // lazily built; AST function declarations by name position
	nonNilCache          map[token.Pos]nilCacheResult // lazily built; proven non-nil results
	crossPkgConverters   map[string]*Converter        // lazily built; cached converters for imported packages
	noCrossPkg           bool                         // when true, skip cross-package transitive analysis
	// Set per-function-conversion: maps `S` to `Slice<E>` for the `S ~[]E` shape.
	typeParamSubstitutions map[string]string
}

func NewConverter(pkgPath string, pkg *packages.Package, cfg *config.Config) *Converter {
	return &Converter{
		currentPkgPath: pkgPath,
		externalPkgs:   make(ExternalPkgs),
		pkg:            pkg,
		cfg:            cfg,
	}
}

func (c *Converter) ExternalPkgs() ExternalPkgs {
	return c.externalPkgs
}

func (c *Converter) trackExternalPkg(pkgPath, pkgName string) {
	if pkgPath != "" && pkgPath != c.currentPkgPath {
		c.externalPkgs[pkgPath] = pkgName
	}
}

func (c *Converter) Convert(symbolExport extract.SymbolExport) ConvertResult {
	result := ConvertResult{
		Name: symbolExport.Name,
		Kind: symbolExport.Kind,
		Doc:  symbolExport.Doc,
	}

	switch symbolExport.Kind {
	case extract.ExportFunction:
		c.convertFunction(&result, symbolExport)
	case extract.ExportMethod:
		c.convertMethod(&result, symbolExport)
	case extract.ExportType:
		c.convertType(&result, symbolExport)
	case extract.ExportConstant:
		c.convertConstant(&result, symbolExport)
	case extract.ExportVariable:
		c.convertVariable(&result, symbolExport)
	}

	return result
}
