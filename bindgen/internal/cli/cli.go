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
	// ExternalImports are the third-party `go:` packages this typedef references.
	ExternalImports []string
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
	_ = fs.Parse(fs.Args()[1:])

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
	targetsFlag := fs.String("targets", "", "comma-separated GOOS/GOARCH list (e.g. linux/amd64,darwin/arm64); falls back to BINDGEN_TARGETS env if unset")

	fs.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: bindgen stdlib -outdir <dir> -targets <list>\n\n")
		fmt.Fprintf(os.Stderr, "Generates .d.lis type definitions for all Go std packages\n")
		fmt.Fprintf(os.Stderr, "across the given targets, deduplicating shared content.\n\n")
		fmt.Fprintf(os.Stderr, "Example:\n")
		fmt.Fprintf(os.Stderr, "  bindgen stdlib -outdir ./outdir -targets linux/amd64,darwin/arm64\n")
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

	targets, err := resolveTargets(*targetsFlag)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: %v\n", err)
		os.Exit(1)
	}

	fmt.Fprintf(os.Stderr, "Generating stdlib bindings to %s...\n", *outDir)

	result, err := GenerateStd(context.Background(), *outDir, effectiveVersion, goVersion, &cfg, targets)
	if err != nil {
		fmt.Fprintf(os.Stderr, "bindgen: %v\n", err)
		os.Exit(1)
	}

	fmt.Fprintf(os.Stderr, "\nGenerated %d package outputs across %d targets in %.1fs\n",
		result.Generated, len(targets), result.Duration.Seconds())
}

func resolveTargets(flagValue string) ([]Target, error) {
	if flagValue != "" {
		return ParseTargets(flagValue)
	}
	if envList := os.Getenv("BINDGEN_TARGETS"); envList != "" {
		return ParseTargets(envList)
	}
	return nil, fmt.Errorf("no targets specified: pass -targets or set BINDGEN_TARGETS (e.g. linux/amd64,darwin/arm64)")
}

func ParseTargets(s string) ([]Target, error) {
	if s == "" {
		return nil, fmt.Errorf("empty target list")
	}
	var targets []Target
	seen := make(map[Target]bool)
	for _, part := range strings.Split(s, ",") {
		part = strings.TrimSpace(part)
		goos, goarch, ok := strings.Cut(part, "/")
		if !ok || goos == "" || goarch == "" || strings.Contains(goarch, "/") {
			return nil, fmt.Errorf("target %q: expected GOOS/GOARCH", part)
		}
		target := Target{goos: goos, goarch: goarch}
		if seen[target] {
			return nil, fmt.Errorf("duplicate target %q", part)
		}
		seen[target] = true
		targets = append(targets, target)
	}
	return targets, nil
}

func generateFromPackage(pkg *packages.Package, displayPath, lisetteVersion, goVersion string, cfg *config.Config, nilness *convert.NilnessAnalysis) GeneratePkgResult {
	converter := convert.NewConverter(pkg.PkgPath, pkg, cfg, nilness)
	exports := extract.ExtractExports(pkg, converter.EmbedIsFaithful)
	converted := converter.ConvertAll(exports)

	closedDomainTypeNames := make(map[string]bool)
	for _, group := range converted.ConstGroups {
		if cfg.IsClosedDomain(pkg.PkgPath, group.Type.Name) {
			closedDomainTypeNames[group.Type.Name] = true
		}
	}

	emitter := emit.NewEmitter(cfg, pkg.PkgPath, pkg.Name, converted.BitFlagSetTypeNames, closedDomainTypeNames)
	emitter.EmitHeader(displayPath, pkg.Name, lisetteVersion, goVersion)

	emitter.EmitImports(converted.ExternalPkgs, converted.NeedsSelfQualification())

	for _, synth := range converted.SyntheticTypes {
		emitter.EmitExport(synth)
	}

	for _, handle := range converted.OpaqueTypes {
		emitter.EmitExport(handle)
	}

	for _, group := range converted.ConstGroups {
		emitter.EmitExport(group.Type)
		for _, constant := range group.Constants {
			emitter.EmitTypedConst(constant, group.Type.Name)
		}
	}

	for _, result := range converted.Symbols {
		if result.SkipReason != nil {
			if result.Kind == extract.ExportMethod && emitter.CollectSkippedMethod(result) {
				continue
			}
			emitter.EmitSkipped(result)
			continue
		}
		emitter.EmitExport(result)
	}

	emitter.EmitImplBlocks()

	externalImports := make([]string, 0, len(converted.ExternalPkgs))
	for path := range converted.ExternalPkgs {
		externalImports = append(externalImports, path)
	}

	return GeneratePkgResult{
		Content:         emitter.String(),
		Summary:         emitter.Summary(),
		ExternalImports: externalImports,
	}
}
