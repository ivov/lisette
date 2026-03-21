package main

import (
	"os"

	"github.com/ivov/lisette/tools/bindgen/internal/cli"
)

func main() {
	if len(os.Args) < 2 {
		cli.PrintUsage()
		os.Exit(2)
	}

	switch os.Args[1] {
	case "pkg":
		cli.RunPkg(os.Args[2:])
	case "stdlib":
		cli.RunStd(os.Args[2:])
	default:
		cli.PrintUsage()
		os.Exit(2)
	}
}
