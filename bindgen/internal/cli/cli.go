package cli

import (
	"context"
	_ "embed"
	"flag"
	"fmt"
	"os"
	"runtime/debug"
	"strings"

	"github.com/ivov/lisette/bindgen/internal/config"
	"github.com/ivov/lisette/bindgen/internal/convert"
	"github.com/ivov/lisette/bindgen/internal/emit"
	"github.com/ivov/lisette/bindgen/internal/extract"
	"golang.org/x/tools/go/packages"
)

type GeneratePkgResult struct {
	Content string
	Summary string
}

var lisVersion = "dev"

//go:embed metadata/go-version
var goVersion string

func init() {
	goVersion = strings.TrimSpace(goVersion)

	// When run via go run ...@vX.Y.Z, the module tag is authoritative.
	if info, ok := debug.ReadBuildInfo(); ok {
		if v := info.Main.Version; v != "" && v != "(devel)" {
			lisVersion = strings.TrimPrefix(v, "v")
		}
	}
}

func PrintUsage() {
	fmt.Fprintf(os.Stderr, "Usage: bindgen <command> [arguments]\n\n")
	fmt.Fprintf(os.Stderr, "Commands:\n")
	fmt.Fprintf(os.Stderr, "  pkg <package>  Generate bindings for a single package\n")
	fmt.Fprintf(os.Stderr, "  pkgs           Generate bindings for many packages (paths on stdin)\n")
	fmt.Fprintf(os.Stderr, "  stdlib -outdir <dir>  Generate bindings for all Go stdlib packages\n")
}

func RunPkg(args []string, defaultCfgJSON []byte) {
	fs := flag.NewFlagSet("pkg", flag.ExitOnError)
	configPath := fs.String("config", "", "path to bindgen config file")
	fs.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: bindgen pkg <package>\n\n")
		fmt.Fprintf(os.Stderr, "Generates .d.lis type definitions for a Go package.\n\n")
		fmt.Fprintf(os.Stderr, "Examples:\n")
		fmt.Fprintf(os.Stderr, "  bindgen pkg fmt                            # Go stdlib\n")
		fmt.Fprintf(os.Stderr, "  bindgen pkg net/http                       # Go stdlib (nested)\n")
		fmt.Fprintf(os.Stderr, "  bindgen pkg golang.org/x/text/transform    # Go extended\n")
		fmt.Fprintf(os.Stderr, "  bindgen pkg github.com/gorilla/mux         # Go community\n")
	}

	_ = fs.Parse(args)

	if fs.NArg() < 1 {
		fs.Usage()
		os.Exit(2)
	}

	pkgPath := fs.Arg(0)

	cfg, err := config.LoadConfig(*configPath, defaultCfgJSON)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to load config: %v\n", err)
		os.Exit(1)
	}

	result, err := GeneratePkg(pkgPath, lisVersion, goVersion, &cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: %v\n", err)
		os.Exit(1)
	}

	fmt.Print(result.Content)

	if stat, _ := os.Stdout.Stat(); (stat.Mode() & os.ModeCharDevice) != 0 {
		fmt.Print(result.Summary)
	}
}

func RunStd(args []string, defaultCfgJSON []byte) {
	fs := flag.NewFlagSet("stdlib", flag.ExitOnError)
	configPath := fs.String("config", "", "path to bindgen config file")
	outDir := fs.String("outdir", "", "output directory for generated .d.lis files")
	version := fs.String("version", "", "override Lisette version in generated headers")
	fs.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: bindgen stdlib -outdir <dir>\n\n")
		fmt.Fprintf(os.Stderr, "Generates .d.lis type definitions for all Go std packages.\n\n")
		fmt.Fprintf(os.Stderr, "Example:\n")
		fmt.Fprintf(os.Stderr, "  bindgen stdlib -outdir ./outdir\n")
	}

	_ = fs.Parse(args)

	if *outDir == "" {
		fs.Usage()
		os.Exit(2)
	}

	cfg, err := config.LoadConfig(*configPath, defaultCfgJSON)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to load config: %v\n", err)
		os.Exit(1)
	}

	effectiveVersion := lisVersion
	if *version != "" {
		effectiveVersion = *version
	}

	fmt.Fprintf(os.Stderr, "Generating stdlib bindings to %s...\n", *outDir)

	result, err := GenerateStd(context.Background(), *outDir, effectiveVersion, goVersion, &cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: %v\n", err)
		os.Exit(1)
	}

	fmt.Fprintf(os.Stderr, "\nGenerated %d packages (%d skipped) in %.1fs\n",
		result.Generated, result.Skipped, result.Duration.Seconds())
}

func generateFromPackage(pkg *packages.Package, displayPath, lisetteVersion, goVersion string, cfg *config.Config) GeneratePkgResult {
	exports := extract.ExtractExports(pkg)

	converter := convert.NewConverter(pkg.PkgPath, pkg, cfg)
	var results []convert.ConvertResult
	for _, exp := range exports {
		results = append(results, converter.Convert(exp))
	}

	valueEnums, constantTypes, valueEnumTypeNames := convert.DetectValueEnums(results, exports)

	enumConstants := make(map[string][]convert.ConvertResult)
	for i, result := range results {
		if typeName, isEnumConstant := constantTypes[i]; isEnumConstant {
			enumConstants[typeName] = append(enumConstants[typeName], result)
		}
	}

	emitter := emit.NewEmitter(cfg, pkg.PkgPath)
	emitter.EmitHeader(displayPath, pkg.Name, lisetteVersion, goVersion)

	emitter.EmitImports(converter.ExternalPkgs())

	for _, ve := range valueEnums {
		emitter.EmitValueEnum(ve)
		for _, constResult := range enumConstants[ve.TypeName] {
			emitter.EmitTypedConst(constResult, ve.TypeName)
		}
	}

	for i, result := range results {
		if _, isEnumConstant := constantTypes[i]; isEnumConstant {
			continue
		}
		if result.Kind == extract.ExportType && valueEnumTypeNames[result.Name] {
			continue
		}
		if result.SkipReason != nil {
			if result.Kind == extract.ExportMethod && emitter.CollectSkippedMethod(result) {
				continue
			}
			emitter.EmitSkipped(exports[i].Name, result.SkipReason)
			continue
		}
		emitter.EmitExport(result)
	}

	emitter.EmitImplBlocks()

	return GeneratePkgResult{
		Content: emitter.String(),
		Summary: emitter.Summary(),
	}
}
