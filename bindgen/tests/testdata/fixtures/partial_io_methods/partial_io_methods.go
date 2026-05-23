package partial_io_methods

import "io"

// Implements io.StringWriter, io.ReaderFrom, io.WriterTo; methods must
// bind as Partial<...>.
type Sink struct{}

func (s *Sink) WriteString(str string) (int, error) { return len(str), nil }
func (s *Sink) ReadFrom(r io.Reader) (int64, error) { return 0, nil }
func (s *Sink) WriteTo(w io.Writer) (int64, error)  { return 0, nil }

// Same names with wrong signatures must stay Result, not Partial.
type NotSink struct{}

func (n *NotSink) WriteString(str string) error         { return nil }
func (n *NotSink) ReadFrom(r io.Reader) (string, error) { return "", nil }
func (n *NotSink) WriteTo(w io.Writer) (string, error)  { return "", nil }
