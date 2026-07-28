// Fixture: defer spills every return through a stack local, and nil-receiver
// guards are unreachable behind a Ref receiver. Proofs must survive both.
package nilness_defer_spill

import (
	"errors"
	"sync"
)

var errFromPanic = errors.New("recovered")

type Config struct {
	mutex sync.RWMutex
	Name  string
}

func (c *Config) Clone() *Config {
	c.mutex.RLock()
	defer c.mutex.RUnlock()
	return &Config{Name: c.Name}
}

func (c *Config) Snapshot() *Config {
	c.mutex.RLock()
	defer c.mutex.RUnlock()
	return &Config{Name: c.Name}
}

func (c *Config) Renamed(name string) *Config {
	if c == nil {
		return nil
	}
	c.mutex.Lock()
	defer c.mutex.Unlock()
	return &Config{Name: name}
}

func mightPanic(c *Config) {
	if c.Name == "" {
		panic("unnamed")
	}
}

func NewChecked(c *Config) *Config {
	defer func() { recover() }()
	mightPanic(c)
	return &Config{Name: c.Name}
}

func WithCleanup(c *Config, cleanup func()) *Config {
	defer cleanup()
	mightPanic(c)
	return &Config{Name: c.Name}
}

func ParseChecked(c *Config) (result *Config, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = errFromPanic
		}
	}()
	mightPanic(c)
	return &Config{Name: c.Name}, nil
}

func FetchNamed(c *Config) (result *Config) {
	defer func() { recover() }()
	mightPanic(c)
	return &Config{Name: c.Name}
}

func ParseLeaky(c *Config) (result *Config, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			if asErr, isErr := recovered.(error); isErr {
				err = asErr
			}
		}
	}()
	mightPanic(c)
	return &Config{Name: c.Name}, nil
}
