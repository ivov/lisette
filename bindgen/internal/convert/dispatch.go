package convert

import (
	"go/ast"
	"go/token"
	"go/types"

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
	// SyntheticType holds a typedef bindgen mints to give an otherwise-skipped
	// var a referenceable type.
	SyntheticType *ConvertResult
}

// HasReturn reports whether this function/method has a non-unit return type
// the caller can observe (i.e. anything other than `()` or absent).
func (r *ConvertResult) HasReturn() bool {
	return r.ReturnType != "" && r.ReturnType != "()"
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
	Name       string
	Type       string
	Doc        string
	SkipReason *SkipReason
}

type InterfaceMethod struct {
	Name        string
	Params      []FunctionParameter
	ReturnType  string
	CommaOk     bool
	ArrayReturn bool
}

// HasReturn reports whether this interface method has a non-unit return type.
func (m *InterfaceMethod) HasReturn() bool {
	return m.ReturnType != "" && m.ReturnType != "()"
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
	currentPkgPath           string
	externalPkgs             ExternalPkgs
	pkg                      *packages.Package
	cfg                      *config.Config
	uniformPointerTypes      map[string]bool              // lazily computed; types with 10+ single-pointer-return methods
	manyToOneTypes           map[string]bool              // lazily computed; return types with 10+ free functions
	majorityPointerTypes     map[string]bool              // lazily computed; types where ≥20 methods return same *T (>90%)
	funcDeclCache            map[token.Pos]*ast.FuncDecl  // lazily built; AST function declarations by name position
	nonNilCache              map[token.Pos]nilCacheResult // lazily built; proven non-nil results
	crossPkgConverters       map[string]*Converter        // lazily built; cached converters for imported packages
	noCrossPkg               bool                         // when true, skip cross-package transitive analysis
	reachableUnexportedTypes map[string]bool              // lazily computed; unexported type names reachable from an exported decl. nil = uncomputed
	shallowUnderlyingCache   map[token.Pos]types.Type     // lazily built; spec-level wrapped type by Named.Obj().Pos(). nil sentinels cached.
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

// shallowUnderlying returns the immediate spec-level wrapped type of a Named
// type by walking its declaring package's syntax. For `type NodeTimeout
// time.Duration` it returns `time.Duration`, not the fully-resolved `int64`
// that types.Type.Underlying would yield. Returns nil when the AST is
// unreachable or the spec is itself a type alias.
func (c *Converter) shallowUnderlying(named *types.Named) types.Type {
	obj := named.Obj()
	if obj == nil || obj.Pkg() == nil || c.pkg == nil {
		return nil
	}
	pos := obj.Pos()
	if c.shallowUnderlyingCache == nil {
		c.shallowUnderlyingCache = make(map[token.Pos]types.Type)
	} else if cached, ok := c.shallowUnderlyingCache[pos]; ok {
		return cached
	}
	resolved := resolveShallowUnderlying(c.pkg.Imports[obj.Pkg().Path()], obj.Name())
	c.shallowUnderlyingCache[pos] = resolved
	return resolved
}

func resolveShallowUnderlying(declPkg *packages.Package, typeName string) types.Type {
	if declPkg == nil || declPkg.TypesInfo == nil {
		return nil
	}
	for _, file := range declPkg.Syntax {
		for _, decl := range file.Decls {
			genDecl, ok := decl.(*ast.GenDecl)
			if !ok || genDecl.Tok != token.TYPE {
				continue
			}
			for _, spec := range genDecl.Specs {
				ts, ok := spec.(*ast.TypeSpec)
				if !ok || ts.Name == nil || ts.Name.Name != typeName {
					continue
				}
				if ts.Assign != token.NoPos {
					return nil
				}
				return declPkg.TypesInfo.TypeOf(ts.Type)
			}
		}
	}
	return nil
}

// salvageInternalAlias rescues a type alias whose RHS is in an internal
// package (Ginkgo's `type NodeTimeout = internal.NodeTimeout` pattern) by
// exposing the immediate wrapped type as a Lisette newtype. Returns the
// newtype payload and true on success.
func (c *Converter) salvageInternalAlias(rhs types.Type, reason *SkipReason) (string, bool) {
	if reason == nil || reason.Code != "internal-package-ref" {
		return "", false
	}
	named, ok := rhs.(*types.Named)
	if !ok {
		return "", false
	}
	shallow := c.shallowUnderlying(named)
	if shallow == nil {
		return "", false
	}
	under := ToLisette(shallow, c)
	if under.SkipReason != nil {
		return "", false
	}
	return under.LisetteType, true
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
