package variables

import "io"

// Package-level variables

// Args holds the command-line arguments.
var Args []string

// Stdin is the standard input.
var Stdin *File

// Stdout is the standard output.
var Stdout *File

// EOF marks end of file.
var EOF error

// Sink is an uninitialized writer.
var Sink io.Writer

// File represents an open file.
type File struct {
	Name string
}

// Counter is a simple counter.
var Counter int

// ConfigMap holds configuration.
var ConfigMap map[string]string
