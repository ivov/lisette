package convert

import "testing"

func mkConsts(values ...string) []constInfo {
	out := make([]constInfo, len(values))
	for i, v := range values {
		out[i] = constInfo{index: i, name: "C", value: v}
	}
	return out
}

func TestLooksLikeBitFlags_SmallSequential_AreEnum(t *testing.T) {
	cases := []struct {
		name string
		vals []string
	}{
		{"{0,1}", []string{"0", "1"}},
		{"{1,2}", []string{"1", "2"}},
		{"{0,1,2}", []string{"0", "1", "2"}},
		{"{1,2,3}", []string{"1", "2", "3"}},
		{"{0,1,2,3,4}", []string{"0", "1", "2", "3", "4"}},
		{"{1,2,3,4}", []string{"1", "2", "3", "4"}},
		{"{1,2,3,4,5}", []string{"1", "2", "3", "4", "5"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if looksLikeBitFlags(mkConsts(c.vals...)) {
				t.Errorf("%s: want enum, got flags", c.name)
			}
		})
	}
}

func TestLooksLikeBitFlags_TextbookFlags(t *testing.T) {
	cases := []struct {
		name string
		vals []string
	}{
		{"{1,2,4,8}", []string{"1", "2", "4", "8"}},
		{"{0,1,2,4,8}", []string{"0", "1", "2", "4", "8"}},
		{"{1,2,4,8,16}", []string{"1", "2", "4", "8", "16"}},
		{"{1,2,4,8,16,32}", []string{"1", "2", "4", "8", "16", "32"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if !looksLikeBitFlags(mkConsts(c.vals...)) {
				t.Errorf("%s: want flags, got enum", c.name)
			}
		})
	}
}

func TestLooksLikeBitFlags_BelowMinConstants(t *testing.T) {
	if looksLikeBitFlags(mkConsts("1", "2", "4")) {
		t.Error("3-value {1,2,4}: want enum (recover via config), got flags")
	}
}

func TestLooksLikeBitFlags_HybridMaskAndBits(t *testing.T) {
	if looksLikeBitFlags(mkConsts("1", "2", "4", "8", "0xff00000")) {
		t.Error("hybrid mask+bits: want enum (recover via config), got flags")
	}
}

func TestIsSequentialRange(t *testing.T) {
	cases := []struct {
		name string
		vals []string
		want bool
	}{
		{"{0,1,2,3,4}", []string{"0", "1", "2", "3", "4"}, true},
		{"{1,2,3,4,5}", []string{"1", "2", "3", "4", "5"}, true},
		{"reversed-order {4,3,2,1,0}", []string{"4", "3", "2", "1", "0"}, true},
		{"{0,1,2,4}", []string{"0", "1", "2", "4"}, false},
		{"{1,2,4,8}", []string{"1", "2", "4", "8"}, false},
		{"starts-at-2 {2,3,4}", []string{"2", "3", "4"}, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := isSequentialRange(mkConsts(c.vals...))
			if got != c.want {
				t.Errorf("%s: want %v, got %v", c.name, c.want, got)
			}
		})
	}
}
