package lisette

import "testing"

func TestMapCloneOfNilIsWritable(t *testing.T) {
	var m map[string]int
	out := MapClone(m)
	if out == nil {
		t.Fatal("MapClone of a nil map returned nil, want an empty map")
	}
	out["k"] = 1
	if out["k"] != 1 {
		t.Errorf("out[%q] = %d, want 1", "k", out["k"])
	}
}

func TestMapCloneCopiesEntries(t *testing.T) {
	m := map[string]int{"a": 1, "b": 2}
	out := MapClone(m)
	out["a"] = 9
	if m["a"] != 1 {
		t.Errorf("clone shares storage: m[%q] = %d, want 1", "a", m["a"])
	}
	if out["b"] != 2 {
		t.Errorf("out[%q] = %d, want 2", "b", out["b"])
	}
}

func TestMapCloneFuncOfNilIsWritable(t *testing.T) {
	var m map[string][]int
	out := MapCloneFunc(m, func(v []int) []int { return append([]int(nil), v...) })
	if out == nil {
		t.Fatal("MapCloneFunc of a nil map returned nil, want an empty map")
	}
	out["k"] = []int{1}
	if len(out["k"]) != 1 {
		t.Errorf("len(out[%q]) = %d, want 1", "k", len(out["k"]))
	}
}

func TestMapCloneFuncClonesValues(t *testing.T) {
	m := map[string][]int{"a": {1, 2}}
	out := MapCloneFunc(m, func(v []int) []int { return append([]int(nil), v...) })
	out["a"][0] = 9
	if m["a"][0] != 1 {
		t.Errorf("value clone shares storage: m[%q][0] = %d, want 1", "a", m["a"][0])
	}
}
