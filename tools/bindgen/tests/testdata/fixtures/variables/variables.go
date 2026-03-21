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

// Discard is a writer that discards all data.
var Discard io.Writer

// File represents an open file.
type File struct {
	Name string
}

// Counter is a simple counter.
var Counter int

// ConfigMap holds configuration.
var ConfigMap map[string]string
