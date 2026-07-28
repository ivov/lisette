// Fixture: (T, error) analysis. A witnessed (nil, nil) return demotes the Ok
// payload to Option; error-exclusivity keeps contract-conforming functions
// and their panicking unwrappers provably non-nil.
package nilness_error_pairs

import (
	"errors"
	"sync"
)

type Entry struct{ Name string }

func NextEntry(done bool) (*Entry, error) {
	if done {
		return nil, nil
	}
	return &Entry{}, nil
}

func NextForwarded(done bool) (*Entry, error) {
	return NextEntry(done)
}

func NextTwoLayers(done bool) (*Entry, error) {
	return NextForwarded(done)
}

func NextGuarded(done bool) (*Entry, error) {
	entry, err := NextEntry(done)
	if err != nil {
		return nil, errors.New("wrapped")
	}
	return entry, nil
}

var forwardMutex sync.Mutex

func NextLocked(done bool) (*Entry, error) {
	forwardMutex.Lock()
	defer forwardMutex.Unlock()
	return NextEntry(done)
}

func LoadEntry(ok bool) (*Entry, error) {
	if !ok {
		return nil, errors.New("missing")
	}
	return &Entry{}, nil
}

func MustLoad(ok bool) *Entry {
	entry, err := LoadEntry(ok)
	if err != nil {
		panic(err)
	}
	return entry
}

func RequireEntry(ok bool) *Entry {
	entry, err := LoadEntry(ok)
	if err != nil {
		panic(err)
	}
	return entry
}

func Passthrough(ok bool) *Entry {
	entry, _ := LoadEntry(ok)
	return entry
}
