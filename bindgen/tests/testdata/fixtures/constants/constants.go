package constants

const (
	IntConst        = 42
	StringConst     = "hello"
	TypedConst  int = 100
)

// Various typed constants
const (
	// Explicitly typed
	MaxInt8  int8    = 127
	MinInt16 int16   = -32768
	Pi       float64 = 3.14159265358979

	// Untyped (will infer type)
	UntypedInt   = 42
	UntypedFloat = 3.14
	UntypedBool  = true

	// Complex expressions
	KB = 1024
	MB = KB * 1024
	GB = MB * 1024
)

// String constants
const (
	Hello   = "hello"
	World   = "world"
	Message = Hello + " " + World
)

// Fixed-size array types
type Buffer [1024]byte
type Matrix [4][4]float64

// Complex numbers
const ComplexVal complex128 = 1 + 2i

// More constants

const (
	// Boolean constant
	DebugMode = true

	// Negative number
	MinValue = -100

	// Iota-based constants
	Sunday = iota
	Monday
	Tuesday
)

// More typed constants
const (
	MaxSize    int64   = 1 << 20
	DefaultTTL float64 = 3600.0
)

// Numeric notation (hex, octal, binary)
const (
	HexConst    = 0xFF
	HexLarge    = 0xDEADBEEF
	OctalConst  = 0o755
	OctalLegacy = 0644
	BinaryConst = 0b1010
	BinaryLong  = 0b11110000
)

// Value enum with negative values (tests parseIntValue handles negatives)
type Priority int

const (
	PriorityLow    Priority = -1
	PriorityNormal Priority = 0
	PriorityHigh   Priority = 1
)

// Negative hex notation (tests parseIntValue handles negative hex)
type Offset int16

const (
	OffsetMin Offset = -0x8000
	OffsetMid Offset = 0
	OffsetMax Offset = 0x7FFF
)
