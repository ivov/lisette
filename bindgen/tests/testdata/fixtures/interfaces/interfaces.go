package interfaces

import (
	"fmt"
	"io"
)

type Empty interface{}

type Stringer interface {
	String() string
}

type ReadWriter interface {
	Read([]byte) (int, error)
	Write([]byte) (int, error)
}

// Error interface - should be detected as error
type MyError interface {
	Error() string
}

// Error-like but with extra method - not an error
type RichError interface {
	Error() string
	Code() int
}

// Embedding standard interfaces
type ReadWriteSeeker interface {
	io.Reader
	io.Writer
	io.Seeker
}

// Empty interface alias
type Any interface{}

// Custom error types

// Custom error type implementing error interface
type ValidationError struct {
	Field   string
	Message string
}

func (e *ValidationError) Error() string {
	return fmt.Sprintf("%s: %s", e.Field, e.Message)
}

// Another error type
type NotFoundError string

func (e NotFoundError) Error() string {
	return string(e)
}

type Node struct {
	Value int
}

// Nilable comma-ok method on interface
type Cache interface {
	Get(key string) (node *Node, ok bool)
	Set(key string, value *Node)
}

// Variadic method on interface
type Logger interface {
	Logf(format string, args ...any)
}

// Function returning custom error
func Validate(s string) error { return nil }
