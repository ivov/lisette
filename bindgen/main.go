package main

import (
	_ "embed"
	"os"

	"github.com/ivov/lisette/bindgen/internal/cli"
)

//go:embed bindgen.external.json
var defaultCfgJSON []byte

func main() {
	if len(os.Args) < 2 {
		cli.PrintUsage()
		os.Exit(2)
	}

	switch os.Args[1] {
	case "pkg":
		cli.RunPkg(os.Args[2:], defaultCfgJSON)
	case "stdlib":
		cli.RunStd(os.Args[2:], defaultCfgJSON)
	default:
		cli.PrintUsage()
		os.Exit(2)
	}
}
