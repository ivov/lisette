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
