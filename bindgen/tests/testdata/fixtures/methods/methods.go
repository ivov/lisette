package methods

// Receivers

type Counter struct {
	Value int
}

func (c Counter) Get() int       { return c.Value }
func (c *Counter) Increment()    { c.Value++ }
func (c *Counter) Add(n int) int { c.Value += n; return c.Value }

// Variadic method - tests convertMethod variadic path
func (c *Counter) Log(format string, args ...any) {}

// Method with unnamed params - tests convertMethod unnamed param path
func (c *Counter) Process(string, int) {}

// Type with anonymous struct field - methods should skip due to receiver type
type BadReceiver struct {
	Data struct{ X int }
}

func (b *BadReceiver) DoSomething() {}

// Name collision - same method name on different types

type A struct{}
type B struct{}

func GetValue() string    { return "" }
func (a A) GetValue() int { return 0 }
func (b B) GetValue() bool { return false }
