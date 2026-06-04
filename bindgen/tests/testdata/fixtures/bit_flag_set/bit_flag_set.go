// Exercises the H13 bit-flag-set detection and the bit_flag_set config override.
package bit_flag_set

// TextbookFlags is a real flag set: every nonzero value is a single bit,
// at least four constants, not sequential. H13 classifies as flags and
// emits #[go(bit_flag_set)] on the bare newtype.
type TextbookFlags uint

const (
	FlagA TextbookFlags = 1 << iota
	FlagB
	FlagC
	FlagD
)

// Color is a small sequential const group; H13 keeps it off the bit-flag path.
type Color int

const (
	Red Color = iota
	Green
	Blue
	Yellow
	Purple
)

type ForcedFlags int

const (
	OptionX ForcedFlags = iota
	OptionY
	OptionZ
)
