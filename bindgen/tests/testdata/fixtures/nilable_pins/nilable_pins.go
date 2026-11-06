// Fixture: pins for values that Go can set to nil where the analysis has no
// proof.
package nilable_pins

type Item struct{ N int }

type Job struct {
	// A pinned error field is Option<error>.
	Err error
	// An error field without a pin stays error.
	Cause error
}

var envs = map[string]map[string]string{}

// A pinned map result is Option.
func Env(key string) map[string]string { return envs[key] }

// A pinned named function type makes its pinned parameters Option.
type Visit func(path string, item *Item, err error) error

type Hooks struct {
	// A pinned function field makes its parameter Option, by position.
	Done func(string, error)
	// The pin does not reach the function type in a parameter.
	Walk func(*Item, func(*Item))
}

// A pin on a parameter makes the parameters of its function type Option.
func Each(items []*Item, f func(*Item) bool) {}

// The pin does not reach the function type in a result.
func Nested(f func(*Item) func(int) int) int { return 0 }
