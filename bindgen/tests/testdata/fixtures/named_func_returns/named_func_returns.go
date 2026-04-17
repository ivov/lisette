package named_func_returns

// Option type for a binding, as in charm.land/bubbles/v2/key.
type BindingOpt func(*Binding)

type Binding struct{ keys []string }

// Factory functions that return BindingOpt must bind as BindingOpt, not Option<BindingOpt>.
func WithKeys(keys ...string) BindingOpt {
	return func(b *Binding) { b.keys = keys }
}

func WithDisabled() BindingOpt {
	return func(b *Binding) {}
}

type Middleware func(Handler) Handler
type Handler func(string) string

func Chain(middlewares ...Middleware) Middleware {
	return func(next Handler) Handler { return next }
}

func MakeRaw() func(int) bool {
	return nil
}
