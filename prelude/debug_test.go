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
