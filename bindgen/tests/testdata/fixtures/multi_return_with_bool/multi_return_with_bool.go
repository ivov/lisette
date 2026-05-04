package multi_return_with_bool

// 2-tuple comma-ok collapses to Option<T>.
func Lookup(key string) (val int, ok bool) { return 0, false }

// 3-tuple ending in bool collapses to Option<(T, U)> by default.
func Collapse3() (a int, b string, ok bool) { return 0, "", false }

// 3-tuple where the trailing bool is metadata, not presence — opted out
// via bool_as_flag.
func Cut3() (before, after string, found bool) { return "", "", false }
