package multi_return_with_bool

// 2-tuple comma-ok collapses to Option<T>.
func Lookup(key string) (val int, ok bool) { return 0, false }

// 3-tuple ending in bool collapses to Option<(T, U)> when every false return gives zero values.
func Collapse3() (a int, b string, ok bool) { return 0, "", false }

// 3-tuple where the trailing bool is metadata, not presence — opted out
// via bool_as_flag.
func Cut3() (before, after string, found bool) { return "", "", false }

var table = map[string]int{}

// A forwarded map lookup collapses to Option<T>.
func Get(key string) (int, bool) {
	v, ok := table[key]
	return v, ok
}

// A forwarded call to a presence function collapses to Option<T>.
func GetTwice(key string) (int, bool) { return Get(key) }

// Named results with a bare return collapse to Option<T>.
func Named(key string) (val int, ok bool) {
	if key != "" {
		val, ok = len(key), true
	}
	return
}

// A value with a false bool stays a tuple.
func Clamp(n int) (int, bool) {
	if n > 10 {
		return 10, false
	}
	return n, true
}

// A bool the analysis cannot follow stays a tuple.
func Unknown(key string) (int, bool) { return len(key), key != "" }

// A pinned bool the analysis cannot follow collapses to Option<T>.
func Pinned(key string) (int, bool) { return len(key), key != "" }

type Source interface {
	// An interface method has no body, so it stays a tuple.
	Next() (int, bool)
	// A pinned interface method collapses to Option<T>.
	Peek() (int, bool)
}

type Options struct {
	// A function type has no body, so it stays a tuple.
	Resolve func(string) (string, bool)
	// A pinned function type collapses to Option<T>.
	Lookup func(string) (string, bool)
}

// A pinned symbol collapses the function types inside it.
func Iterate() (next func() (int, bool)) { return nil }

// A generic zero value collapses to Option<T>.
func First[T any](s []T) (T, bool) {
	if len(s) == 0 {
		var zero T
		return zero, false
	}
	return s[0], true
}

type Stack[T any] struct{ items []T }

// A method on a generic type collapses to Option<T>.
func (s *Stack[T]) Pop() (T, bool) {
	var zero T
	if len(s.items) == 0 {
		return zero, false
	}
	top := s.items[len(s.items)-1]
	s.items = s.items[:len(s.items)-1]
	return top, true
}

func triple() (int, bool, bool) { return 42, false, true }

// A forwarded bool that is not the callee's last result stays a tuple.
func Prefix() (int, bool) {
	n, ok, _ := triple()
	return n, ok
}

type Reader interface {
	// A pinned interface method collapses the function types inside it.
	Next() func() (int, bool)
}

type Flags struct {
	// A flag pin wins over a presence pin on a function type.
	Read func() (int, bool)
}

type Item struct{ N int }

// A nil value with a true bool stays a tuple.
func NilPresent() (*Item, bool) { return nil, true }

type Finder interface {
	// A nilable value next to a bool with no body stays Option.
	Find() (*Item, bool)
}

type Cursor struct{ n int }

// A method follows the interface method it implements, so it stays a tuple.
func (c Cursor) Next() (int, bool) { return 0, false }

// A method follows the pinned interface method it implements, so it collapses to Option<T>.
func (c Cursor) Peek() (int, bool) { return c.n, c.n > 0 }

type Status bool

// A defined bool type stays a tuple.
func ReadStatus() (int, Status) { return 0, false }

var nextItem = func() (*Item, bool) { return nil, false }

// A nilable value forwarded from a function value stays Option.
func Forward() (*Item, bool) { return nextItem() }

type Handler func() int

type HandlerAlias = Handler

type Handlers interface {
	// A named function value next to a bool with no body stays Option.
	Named() (Handler, bool)
	// An alias of a named function type stays Option too.
	Aliased() (HandlerAlias, bool)
	// A bare function value next to a bool with no body stays Option.
	Bare() (func() int, bool)
}
