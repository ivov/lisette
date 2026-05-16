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

// Color is a small sequential enum. H13 emits as `pub enum`.
type Color int

const (
	Red Color = iota
	Green
	Blue
	Yellow
	Purple
)

// ForcedFlags is sequential 0..2 and would be a value enum under H13,
// but the fixture bindgen.json forces it via the bit_flag_set override.
type ForcedFlags int

const (
	OptionX ForcedFlags = iota
	OptionY
	OptionZ
)
