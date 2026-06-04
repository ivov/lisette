// Exercises the closed_domain config override: a curated named primitive whose
// valid values are a fixed finite set, emitting #[go(closed_domain)] on the newtype.
package closed_domain

// Weekday is a closed domain over a fixed set of days. The closed_domain
// override emits #[go(closed_domain)] so the out_of_domain_value lint can fire.
type Weekday int

const (
	Sunday Weekday = iota
	Monday
	Tuesday
	Wednesday
	Thursday
	Friday
	Saturday
)

// Plain is a sequential const group with no override; it stays an ordinary
// named primitive with no closed marker.
type Plain int

const (
	One Plain = iota
	Two
	Three
)

// Flagish has bit-flag-shaped values (single bits, four constants) that the H13
// heuristic would route to #[go(bit_flag_set)]. The closed_domain override must
// win, so it emits #[go(closed_domain)] instead.
type Flagish int

const (
	FlagOne   Flagish = 1
	FlagTwo   Flagish = 2
	FlagFour  Flagish = 4
	FlagEight Flagish = 8
)
