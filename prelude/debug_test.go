package lisette

import "testing"

type valueReceiverDebug struct{ x int }

func (v valueReceiverDebug) DebugString() string { return "vd" }

func TestDebugTypedNilPointerDoesNotPanic(t *testing.T) {
	var p *valueReceiverDebug
	if got := Debug(p); got != "nil" {
		t.Fatalf("Debug(typed nil) = %q, want \"nil\"", got)
	}
	v := valueReceiverDebug{x: 1}
	if got := Debug(&v); got != "vd" {
		t.Fatalf("Debug(&value) = %q, want \"vd\"", got)
	}
}

func TestUnitDisplaysAsParens(t *testing.T) {
	cases := []struct{ got, want string }{
		{MakeOptionSome(struct{}{}).String(), "Some(())"},
		{MakeResultOk[struct{}, string](struct{}{}).String(), "Ok(())"},
		{MakeResultErr[int](struct{}{}).String(), "Err(())"},
		{Partial[struct{}, struct{}]{Tag: PartialBoth}.String(), "Both((), ())"},
		{MakeTuple2(struct{}{}, 1).String(), "((), 1)"},
		{MakeOptionSome(MakeOptionSome(struct{}{})).String(), "Some(Some(()))"},
		{MakeOptionSome(struct{}{}).DebugString(), "Some(())"},
		{Debug([]struct{}{{}, {}}), "[(), ()]"},
	}
	for _, c := range cases {
		if c.got != c.want {
			t.Errorf("got %q, want %q", c.got, c.want)
		}
	}
}
