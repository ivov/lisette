package cli

import (
	"context"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/ivov/lisette/tools/bindgen/internal/config"
	"github.com/ivov/lisette/tools/bindgen/internal/convert"
	"github.com/ivov/lisette/tools/bindgen/internal/emit"
	"github.com/ivov/lisette/tools/bindgen/internal/extract"
	"golang.org/x/tools/go/packages"
)

type GeneratePkgResult struct {
	Content string
	Summary string
}

var (
	lisVersion = "dev"
	goVersion  = "unknown"
)

func init() {
	if lisVer, goVer, err := loadCargoToml(); err == nil {
		lisVersion = lisVer
		goVersion = goVer
	}
}

func loadCargoToml() (lisVer, goVer string, err error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", "", err
	}

	for {
		path := filepath.Join(dir, "Cargo.toml")
		if _, err := os.Stat(path); err == nil {
			content, err := os.ReadFile(path)
			if err != nil {
				return "", "", err
			}
			lisVer = extractTomlValue(string(content), "[workspace.package]", "version")
			goVer = extractTomlValue(string(content), "[workspace.metadata.lisette]", "go-version")
			return lisVer, goVer, nil
		}

		parent := filepath.Dir(dir)
		if parent == dir {
			return "", "", fmt.Errorf("failed to find Cargo.toml")
		}
		dir = parent
	}
}

func extractTomlValue(content, section, key string) string {
	_, after, ok1 := strings.Cut(content, section)
	if !ok1 {
		return ""
	}

	afterSection := after

	before, _, ok2 := strings.Cut(afterSection, "\n[")
	var sectionContent string
	if !ok2 {
		sectionContent = afterSection
	} else {
		sectionContent = before
	}

	re := regexp.MustCompile(regexp.QuoteMeta(key) + `\s*=\s*"([^"]+)"`)
	match := re.FindStringSubmatch(sectionContent)
	if len(match) < 2 {
		return ""
	}
	return match[1]
}

func PrintUsage() {
	fmt.Fprintf(os.Stderr, "Usage: bindgen <command> [arguments]\n\n")
	fmt.Fprintf(os.Stderr, "Commands:\n")
	fmt.Fprintf(os.Stderr, "  pkg <package>  Generate bindings for a single package\n")
	fmt.Fprintf(os.Stderr, "  std -outdir <dir>  Generate bindings for all Go stdlib packages\n")
}

func RunPkg(args []string) {
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

	cfg, err := config.LoadConfig(*configPath)
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

func RunStd(args []string) {
	fs := flag.NewFlagSet("stdlib", flag.ExitOnError)
	configPath := fs.String("config", "", "path to bindgen config file")
	outDir := fs.String("outdir", "", "output directory for generated .d.lis files")
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

	cfg, err := config.LoadConfig(*configPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: failed to load config: %v\n", err)
		os.Exit(1)
	}

	fmt.Fprintf(os.Stderr, "Generating stdlib bindings to %s...\n", *outDir)

	result, err := GenerateStd(context.Background(), *outDir, lisVersion, goVersion, &cfg)
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
	emitter.EmitHeader(displayPath, lisetteVersion, goVersion)

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
