// Fixture: mutation pins enter the write summaries, so callers inherit them.
package mutation_pins

import "sync"

type Gauge struct {
	v  int
	mu sync.Mutex
}

func (g *Gauge) Set(v int) {}

// Inherits the pinned receiver write.
func (g *Gauge) Reset() { g.Set(0) }

// Locking a mutex is not a write.
func (g *Gauge) Read() int {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.v
}

func Fill(buf []byte) {}

// Inherits the pinned parameter write.
func FillAll(buf []byte) { Fill(buf) }

// A pinned variadic out-parameter.
func Scan(a ...any) {}

// A pointer in a pinned variadic tail is not followed yet.
func ScanOne(p *int) { Scan(p) }

func Peek(buf []byte) { buf[0] = 0 }

// Does not inherit the write the pin removes.
func PeekAll(buf []byte) { Peek(buf) }

type Cache struct{ last *int }

// Writes only a cache.
func (c *Cache) Lookup() int {
	x := 1
	c.last = &x
	return x
}

// Does not inherit the receiver write.
func (c *Cache) Get() int { return c.Lookup() }

type Copier struct{}

func (*Copier) Copy(dst, src []byte) { copy(dst, src) }

// Keeps the write to a.
func BoundCopy(c *Copier, a, b []byte) {
	f := c.Copy
	f(a, b)
}

type Box struct{ n int }

func (*Box) Put(v []byte) {}

// Gives the write to b, not v.
func BoundPut(b *Box, v []byte) {
	f := b.Put
	f(v)
}

type Memo struct{ cached int }

// Recursive, with a helper that sorts after the method.
func (c *Memo) A(n int, out []int) int {
	c.cached = 42
	out[0] = n
	if n > 0 {
		return ZQuery(c, n-1, out)
	}
	return 42
}

func ZQuery(c *Memo, n int, out []int) int { return c.A(n, out) }

type Tally struct{ cached int }

// Recursive, with a helper that sorts before the method.
func (c *Tally) A(n int, out []int) int {
	c.cached = 42
	out[0] = n
	if n > 0 {
		return B(c, n-1, out)
	}
	return 42
}

func B(c *Tally, n int, out []int) int { return c.A(n, out) }
