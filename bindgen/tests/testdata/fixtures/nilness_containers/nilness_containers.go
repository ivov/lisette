// Fixture: a map or channel return is bare unless the body proves a nil path,
// unlike a pointer return, which is Option unless proven non-nil. A nil slice
// stays bare either way, since Go reads and ranges it as empty.
package nilness_containers

type Registry map[string]int

func Lookup(enabled bool) map[string]int {
	if enabled {
		return map[string]int{"a": 1}
	}
	return nil
}

func Table() map[string]int {
	return map[string]int{}
}

func Named(enabled bool) Registry {
	if enabled {
		return Registry{}
	}
	return nil
}

func Events(enabled bool) chan int {
	if enabled {
		return make(chan int)
	}
	return nil
}

func Inbox() <-chan int {
	return make(chan int)
}

func Outbox(enabled bool) chan<- int {
	if enabled {
		return make(chan int)
	}
	return nil
}

func Items(enabled bool) []int {
	if enabled {
		return []int{1}
	}
	return nil
}

func Pair(enabled bool) (map[string]int, error) {
	if enabled {
		return map[string]int{}, nil
	}
	return nil, nil
}

func Delegating(m map[string]int) map[string]int {
	if m == nil {
		return nil
	}
	return m
}

func Forwarded(enabled bool) map[string]int {
	return Lookup(enabled)
}

func Guarded(enabled bool) (map[string]int, bool) {
	if enabled {
		return map[string]int{}, true
	}
	return nil, false
}
