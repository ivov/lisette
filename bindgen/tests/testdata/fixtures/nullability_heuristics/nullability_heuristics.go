package nullability_heuristics

type Widget struct{ ID int }

func Alpha() *Widget   { return &Widget{} }
func Beta() *Widget    { return &Widget{} }
func Gamma() *Widget   { return &Widget{} }
func Delta() *Widget   { return &Widget{} }
func Epsilon() *Widget { return &Widget{} }
func Zeta() *Widget    { return &Widget{} }
func Eta() *Widget     { return &Widget{} }
func Theta() *Widget   { return &Widget{} }
func Iota() *Widget    { return &Widget{} }
func Kappa() *Widget   { return &Widget{} }
func Lambda() *Widget  { return &Widget{} }

type Gadget struct{ Name string }

func SmallA() *Gadget { return &Gadget{} }
func SmallB() *Gadget { return nil }

type Builder struct{ buf []byte }

func (b *Builder) Reset() *Builder                 { b.buf = nil; return b }
func (b *Builder) Append(data []byte) *Builder     { b.buf = append(b.buf, data...); return b }
func (b *Builder) SetTag(key, val string) *Builder { return b }

var defaultBuilder = &Builder{}

func Reset() *Builder                 { return defaultBuilder.Reset() }
func Append(data []byte) *Builder     { return defaultBuilder.Append(data) }
func SetTag(key, val string) *Builder { return defaultBuilder.SetTag(key, val) }

func GetDefault() *Builder { return nil }

type TreeIterator struct{ pos int }
type ItemIterator struct{ pos int }

func Scan() *TreeIterator                  { return &TreeIterator{} }
func (t *TreeIterator) Sub() *ItemIterator { return &ItemIterator{} }

type Cursor struct{ pos int }

func ScanCursor() *Cursor { return nil }

type Store struct{ data map[string]string }

func (s *Store) Fetch(key string) *Result { return nil }

type Result struct{ Value string }

func Fetch(key string) *Result { return nil }

type Knob struct{ name string }

func MakeKnob() *Knob {
	p := new(Knob)
	return p
}

func DirectMakeKnob(name string) *Knob {
	return MakeKnob()
}

func IndirectMakeKnob(name string) *Knob {
	k := MakeKnob()
	return k
}
