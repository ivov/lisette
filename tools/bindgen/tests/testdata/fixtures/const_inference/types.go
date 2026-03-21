package constinference

// FileMode is a custom type for file mode bits
type FileMode int32

// These constants are untyped in Go but should be inferred as FileMode
// because they're assigned from other FileMode-typed constants
const (
	ModePerm = FileMode(0o777)
	ModeDir  = FileMode(0o40000)
)

// Typed constants should remain as their declared type
const (
	TypedConst FileMode = 0o644
)
