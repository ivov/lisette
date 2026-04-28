package config

import (
	"encoding/json"
	"os"
	"slices"
	"strings"
)

// Config holds bindgen configuration loaded from a bindgen JSON config file.
type Config struct {
	Overrides Overrides `json:"overrides"`
}

// Overrides holds all override configurations.
type Overrides struct {
	Lints LintOverrides `json:"lints"`
	Types TypeOverrides `json:"types"`
}

// LintOverrides holds lint-suppression overrides.
type LintOverrides struct {
	AllowUnusedResult map[string][]string `json:"allow_unused_result"`
	DenyUnusedValue   map[string][]string `json:"deny_unused_value"`
}

// TypeOverrides holds type-conversion overrides.
type TypeOverrides struct {
	NilableReturn    map[string][]string            `json:"nilable_return"`
	NonNilableReturn map[string][]string            `json:"non_nilable_return"`
	NonNilableVar    map[string][]string            `json:"non_nilable_var"`
	BoolAsFlag       map[string][]string            `json:"bool_as_flag"`
	DirectError      map[string][]string            `json:"direct_error"`
	NilableError     map[string][]string            `json:"nilable_error"`
	NeverReturn      map[string][]string            `json:"never_return"`
	PartialResult    map[string][]string            `json:"partial_result"`
	MutatesParam     map[string]map[string][]string `json:"mutates_param"`
	// SentinelMinusOne declares int-returning functions that signal
	// "absent" with `-1`. Bindgen rewrites the return to `Option<int>`
	// and emits `#[go(sentinel_minus_one)]`.
	SentinelMinusOne map[string][]string `json:"sentinel_minus_one"`
	// ReflectionDecode declares functions whose `interface{}` params reach
	// Go reflection; each such param is lifted to a fresh `T` and rewritten
	// to `Ref<T>`.
	ReflectionDecode map[string][]string `json:"reflection_decode"`
}

// LoadConfig loads bindgen configuration from the given path.
// Falls back to defaultData when configPath is empty.
func LoadConfig(configPath string, defaultData []byte) (Config, error) {
	var data []byte
	if configPath != "" {
		var err error
		data, err = os.ReadFile(configPath)
		if err != nil {
			return Config{}, err
		}
	} else if len(defaultData) > 0 {
		data = defaultData
	} else {
		return Config{}, nil
	}

	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return Config{}, err
	}
	return cfg, nil
}

// ShouldAllowUnusedResult returns true if the given function in the given
// package should be annotated with #[allow(unused_result)].
//
// Supports wildcards:
//   - "*" matches all functions and methods in the package
//   - "*.Method" matches Method on any type (e.g., "*.Write" for all Writer types)
func (c *Config) ShouldAllowUnusedResult(pkg, funcName string) bool {
	funcs, ok := lookupWithGlob(c.Overrides.Lints.AllowUnusedResult, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(funcs, funcName)
}

// ShouldDenyUnusedValue forces the AST fluent-method heuristic off for curated methods that match its shape but semantically return new values.
func (c *Config) ShouldDenyUnusedValue(pkg, name string) bool {
	if c == nil {
		return false
	}
	names, ok := lookupWithGlob(c.Overrides.Lints.DenyUnusedValue, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(names, name)
}

// ShouldWrapNilableReturn returns true if the given function or method in the given
// package should be wrapped in Option<> because it can return nil.
// Uses "Type.Method" dot notation for methods.
func (c *Config) ShouldWrapNilableReturn(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.NilableReturn, pkg)
	if !ok {
		return false
	}
	return slices.Contains(names, name)
}

// IsNonNilableReturnreturns true if the given function or method in the given
// package is known to never return nil, suppressing automatic Option<> wrapping.
// Uses "Type.Method" dot notation for methods.
//
// Supports wildcards:
//   - "*" matches all functions and methods in the package
//   - "*.Method" matches Method on any type (e.g., "*.Header" for all RR types)
func (c *Config) IsNonNilableReturn(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.NonNilableReturn, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(names, name)
}

// IsNonNilableVar returns true if the given package-level variable in the given
// package is known to always be initialized, suppressing automatic Option<> wrapping.
func (c *Config) IsNonNilableVar(pkg, varName string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.NonNilableVar, pkg)
	if !ok {
		return false
	}
	return slices.Contains(names, "*") || slices.Contains(names, varName)
}

// HasBoolAsFlag returns true if the given function or method returns (T, bool)
// where the bool is a flag (not presence), so it should NOT be converted to Option<T>.
// Uses "Type.Method" dot notation for methods.
func (c *Config) HasBoolAsFlag(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.BoolAsFlag, pkg)
	if !ok {
		return false
	}
	return slices.Contains(names, name)
}

// MutatingParams returns the list of parameter names that are mutated by the
// given function or method, or nil if none are configured.
func (c *Config) MutatingParams(pkg, name string) []string {
	if c == nil {
		return nil
	}
	funcs, ok := lookupWithGlobNested(c.Overrides.Types.MutatesParam, pkg)
	if !ok {
		return nil
	}
	return funcs[name] // nil if not found
}

// IsPartialResult returns true if the given function or method in the given
// package returns (T, error) where both values may be simultaneously meaningful,
// so the return type should be Partial<T, error> instead of Result<T, error>.
func (c *Config) IsPartialResult(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.PartialResult, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(names, name)
}

// HasDirectError returns true if the given function returns error as a value
// (e.g., errors.New), not as a fallible indicator. These should return `error`
// directly instead of `Result<(), error>`.
func (c *Config) HasDirectError(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.DirectError, pkg)
	if !ok {
		return false
	}
	return slices.Contains(names, name)
}

// HasNilableError returns true if the given function returns error as an
// optional value (e.g., errors.Unwrap), where nil means "absent" rather than
// "success". These should return `Option<error>` instead of `Result<(), error>`.
func (c *Config) HasNilableError(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.NilableError, pkg)
	if !ok {
		return false
	}
	return slices.Contains(names, name)
}

// SentinelInt returns (value, true) when the given function signals
// "absent" with a magic int (e.g. -1 for strings.Index).
func (c *Config) SentinelInt(pkg, name string) (int, bool) {
	if c == nil {
		return 0, false
	}
	if names, ok := lookupWithGlob(c.Overrides.Types.SentinelMinusOne, pkg); ok && matchesWildcard(names, name) {
		return -1, true
	}
	return 0, false
}

// IsReflectionDecode reports whether the given function or method is
// configured to lift its `interface{}` params to `Ref<T>`. Uses "Type.Method"
// dot notation for methods.
func (c *Config) IsReflectionDecode(pkg, name string) bool {
	if c == nil {
		return false
	}
	names, ok := lookupWithGlob(c.Overrides.Types.ReflectionDecode, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(names, name)
}

// IsNeverReturn returns true if the given function or method in the given
// package never returns normally (e.g., os.Exit, log.Fatal).
func (c *Config) IsNeverReturn(pkg, name string) bool {
	names, ok := lookupWithGlob(c.Overrides.Types.NeverReturn, pkg)
	if !ok {
		return false
	}
	return matchesWildcard(names, name)
}

// lookupWithGlob returns all matching names for a package from a map,
// combining exact matches with glob pattern matches. Keys ending in "/**"
// match any package under that prefix (e.g., "cloud.google.com/go/**"
// matches "cloud.google.com/go/storage").
func lookupWithGlob(m map[string][]string, pkg string) ([]string, bool) {
	exact := m[pkg]
	var merged []string
	for key, names := range m {
		if strings.HasSuffix(key, "/**") {
			prefix := key[:len(key)-2] // keep trailing "/"
			if strings.HasPrefix(pkg, prefix) {
				if merged == nil {
					merged = append(merged, exact...)
				}
				merged = append(merged, names...)
			}
		}
	}
	if merged != nil {
		return merged, true
	}
	return exact, len(exact) > 0
}

// lookupWithGlobNested is like lookupWithGlob but for map[string]map[string][]string.
// It merges function-level maps from exact and glob matches.
func lookupWithGlobNested(m map[string]map[string][]string, pkg string) (map[string][]string, bool) {
	exact := m[pkg]
	var merged map[string][]string
	for key, funcs := range m {
		if strings.HasSuffix(key, "/**") {
			prefix := key[:len(key)-2]
			if strings.HasPrefix(pkg, prefix) {
				if merged == nil {
					merged = make(map[string][]string)
					for k, v := range exact {
						merged[k] = slices.Clone(v)
					}
				}
				for fn, params := range funcs {
					merged[fn] = append(merged[fn], params...)
				}
			}
		}
	}
	if merged != nil {
		return merged, true
	}
	return exact, len(exact) > 0
}

// matchesWildcard checks if name matches any entry in names, supporting:
//   - "*" matches everything
//   - "*.Method" matches "AnyType.Method"
//   - exact match
func matchesWildcard(names []string, name string) bool {
	if slices.Contains(names, "*") || slices.Contains(names, name) {
		return true
	}
	if dot := strings.IndexByte(name, '.'); dot >= 0 {
		methodName := name[dot+1:]
		if slices.Contains(names, "*."+methodName) {
			return true
		}
	}
	return false
}
