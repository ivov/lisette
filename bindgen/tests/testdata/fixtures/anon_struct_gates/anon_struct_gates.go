package anon_struct_gates

// A field tag is part of Go's struct type identity and Lisette cannot yet carry
// it, so a synthesized type would not be assignable. Tagged anonymous structs
// must keep skipping with the anonymous-struct reason.

// TaggedReturn skips: the field carries a json tag.
func TaggedReturn() struct {
	X int `json:"x"`
} {
	return struct {
		X int `json:"x"`
	}{}
}

// TaggedParam skips: the same gate in parameter position drops the whole function.
func TaggedParam(p struct {
	X int `json:"x"`
}) {
}

// An unexported field cannot be reproduced in another package, so the
// synthesized underlying type would differ. These must skip too.

// UnexportedReturn skips: its only field is unexported.
func UnexportedReturn() struct{ secret int } { return struct{ secret int }{} }

// PartlyExported skips whole: mixing exported and unexported fields still
// differs from Go's underlying type, so the struct is rejected, not partially
// bound.
func PartlyExported() struct {
	Public  int
	private int
} {
	return struct {
		Public  int
		private int
	}{}
}

// OrphanSibling skips whole (the array param is unrepresentable). Its first
// param mints a uniquely-shaped synthesized struct; that synth must not leak
// into the output as an orphan once the function is dropped.
func OrphanSibling(p struct{ OnlyHere int }, addr [4]byte) {}

// In a named struct each gated anonymous-struct field drops while the rest stay.
type Mix struct {
	Tagged struct {
		X int `json:"x"`
	}
	Hidden struct{ secret int }
	OK     string
}
