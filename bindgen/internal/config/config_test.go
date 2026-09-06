package config

import "testing"

func TestShouldTreatAsBitFlagSet(t *testing.T) {
	cfg := &Config{
		Overrides: Overrides{
			Types: TypeOverrides{
				BitFlagSet: map[string][]string{
					"io/fs":     {"FileMode"},
					"debug/elf": {"DynamicVersionFlag", "ProgFlag", "SectionFlag"},
				},
			},
		},
	}

	cases := []struct {
		pkg      string
		typeName string
		want     bool
	}{
		{"io/fs", "FileMode", true},
		{"debug/elf", "ProgFlag", true},
		{"debug/elf", "SectionFlag", true},
		{"debug/elf", "ProgType", false}, // not listed
		{"io/fs", "FileInfo", false},     // not listed
		{"net/http", "FileMode", false},  // wrong package
	}
	for _, c := range cases {
		got := cfg.ShouldTreatAsBitFlagSet(c.pkg, c.typeName)
		if got != c.want {
			t.Errorf("ShouldTreatAsBitFlagSet(%q, %q) = %v, want %v", c.pkg, c.typeName, got, c.want)
		}
	}
}

func TestShouldTreatAsBitFlagSet_NilConfig(t *testing.T) {
	var cfg *Config
	if cfg.ShouldTreatAsBitFlagSet("io/fs", "FileMode") {
		t.Error("nil config should return false")
	}
}

func TestNilConfigAccessorsDoNotPanic(t *testing.T) {
	var cfg *Config

	if cfg.ShouldAllowUnusedResult("io", "Foo") {
		t.Error("ShouldAllowUnusedResult")
	}
	if cfg.ShouldAllowUnusedValue("io", "Foo") {
		t.Error("ShouldAllowUnusedValue")
	}
	if cfg.ShouldDenyUnusedValue("io", "Foo") {
		t.Error("ShouldDenyUnusedValue")
	}
	if cfg.ShouldWrapNilableReturn("io", "Foo") {
		t.Error("ShouldWrapNilableReturn")
	}
	if cfg.IsNonNilableReturn("io", "Foo") {
		t.Error("IsNonNilableReturn")
	}
	if cfg.IsNonNilableVar("io", "Foo") {
		t.Error("IsNonNilableVar")
	}
	if cfg.HasBoolAsFlag("io", "Foo") {
		t.Error("HasBoolAsFlag")
	}
	if cfg.MutatingParams("io", "Foo") != nil {
		t.Error("MutatingParams")
	}
	if cfg.NonMutatingParams("io", "Foo") != nil {
		t.Error("NonMutatingParams")
	}
	if cfg.NilableParams("io", "Foo") != nil {
		t.Error("NilableParams")
	}
	if cfg.IsPartialResult("io", "Foo") {
		t.Error("IsPartialResult")
	}
	if cfg.HasDirectError("io", "Foo") {
		t.Error("HasDirectError")
	}
	if cfg.HasNilableError("io", "Foo") {
		t.Error("HasNilableError")
	}
	if _, ok := cfg.SentinelInt("io", "Foo"); ok {
		t.Error("SentinelInt")
	}
	if cfg.IsReflectionDecode("io", "Foo") {
		t.Error("IsReflectionDecode")
	}
	if cfg.IsNeverReturn("io", "Foo") {
		t.Error("IsNeverReturn")
	}
	if cfg.IsClosedDomain("io", "Foo") {
		t.Error("IsClosedDomain")
	}
	if _, ok := cfg.ViewOverrides("io", "Foo"); ok {
		t.Error("ViewOverrides")
	}
	if cfg.HasWritableReturn("io", "Foo") {
		t.Error("HasWritableReturn")
	}
	if _, ok := cfg.SupersededBy("io", "Foo"); ok {
		t.Error("SupersededBy")
	}
}

func TestSupersededBy(t *testing.T) {
	cfg := &Config{
		Overrides: Overrides{
			Types: TypeOverrides{
				SupersededBy: map[string]map[string]string{
					"sort": {
						"Ints":  "slices.Sort",
						"Slice": "slices.SortFunc",
					},
				},
			},
		},
	}

	cases := []struct {
		pkg  string
		name string
		want string
		ok   bool
	}{
		{"sort", "Ints", "slices.Sort", true},
		{"sort", "Slice", "slices.SortFunc", true},
		{"sort", "Search", "", false}, // not listed
		{"slices", "Ints", "", false}, // wrong package
	}
	for _, c := range cases {
		got, ok := cfg.SupersededBy(c.pkg, c.name)
		if got != c.want || ok != c.ok {
			t.Errorf("SupersededBy(%q, %q) = %q, %v; want %q, %v", c.pkg, c.name, got, ok, c.want, c.ok)
		}
	}
}

func TestViewOverridesDistinguishEmptyFromAbsent(t *testing.T) {
	cfg := &Config{
		Overrides: Overrides{
			Types: TypeOverrides{
				ReturnsViewOf: map[string]map[string][]string{
					"syscall": {
						"Aliasing": {"0:buf", "1:[recv]"},
						"Fresh":    {},
					},
				},
			},
		},
	}

	entries, ok := cfg.ViewOverrides("syscall", "Aliasing")
	if !ok || len(entries) != 2 {
		t.Errorf("Aliasing: got %v, %v", entries, ok)
	}
	entries, ok = cfg.ViewOverrides("syscall", "Fresh")
	if !ok || len(entries) != 0 {
		t.Errorf("Fresh: got %v, %v", entries, ok)
	}
	if _, ok := cfg.ViewOverrides("syscall", "Absent"); ok {
		t.Error("Absent: expected no entry")
	}
	if _, ok := cfg.ViewOverrides("os", "Aliasing"); ok {
		t.Error("wrong package: expected no entry")
	}
}
