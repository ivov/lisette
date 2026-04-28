// Fixtures for the builder-method heuristic that emits #[allow(unused_value)]
// when a pointer-receiver method's body returns the receiver itself or
// delegates via a method call on the receiver.
package builder_methods

// Self-return on pointer receiver: classic fluent setter shape.
type Config struct {
	name string
	tags []string
}

func (c *Config) WithName(n string) *Config { c.name = n; return c }
func (c *Config) WithTag(k, v string)       {} // no return: not a builder
func (c *Config) Name() string              { return c.name }

// Interface self-return on a real fluent setter (mutates state, returns self).
type Router interface {
	Get(path string) Router
	Post(path string) Router
}

type Server struct{ routes []string }

func (s *Server) Get(path string) Router  { s.routes = append(s.routes, "GET "+path); return s }
func (s *Server) Post(path string) Router { s.routes = append(s.routes, "POST "+path); return s }

// Delegation: body returns a method call on the receiver. Common in Fiber's
// `func (app *App) Get(...) Router { return app.Add(...) }` shape.
type App struct{ server *Server }

func (a *App) Add(method, path string) *App {
	a.server.routes = append(a.server.routes, method+" "+path)
	return a
}
func (a *App) GetRoute(path string) *App  { return a.Add("GET", path) }
func (a *App) PostRoute(path string) *App { return a.Add("POST", path) }

// Pointer receiver returning a *different* concrete type — must not be marked.
type Group struct{}

func (a *App) NewGroup() *Group { return &Group{} }

// Value receiver returning same type — must not be marked (immutable update).
type Coord struct{ X, Y int }

func (c Coord) Translate(dx, dy int) Coord { return Coord{c.X + dx, c.Y + dy} }

// Clone/Copy return a new value — must not be marked even though the body
// returns the receiver name in some path; the Clone/Copy name exclusion catches
// the common case before AST inspection runs.
type Cfg struct{ tags []string }

func (c *Cfg) Clone() *Cfg { return &Cfg{tags: append([]string(nil), c.tags...)} }
func (c *Cfg) Copy() *Cfg  { return c.Clone() }

// Trivial single-statement return-self getter — must not be marked. This is
// the structural shape of interface-method implementations like
// `func (b *BasicType) Basic() *BasicType { return b }` in go/debug/dwarf.
type TypeLike interface {
	Underlying() TypeLike
	Self() TypeLike
}

type Box struct{}

func (b *Box) Underlying() TypeLike { return b }
func (b *Box) Self() TypeLike       { return b }

// Copy-and-modify pattern (slog.Logger.With shape) — must not be marked.
// Body assigns to a local and returns the local.
type Logger struct{ attrs []string }

func (l *Logger) With(attr string) *Logger {
	clone := &Logger{attrs: append([]string(nil), l.attrs...)}
	clone.attrs = append(clone.attrs, attr)
	return clone
}

// Multi-path method where one return path is non-receiver (go/types Origin
// shape) — must not be marked because not ALL returns are receiver-like.
type Var struct{ origin *Var }

func (v *Var) Origin() *Var {
	if v.origin != nil {
		return v.origin
	}
	return v
}
