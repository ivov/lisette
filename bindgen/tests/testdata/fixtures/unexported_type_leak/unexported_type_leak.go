package unexportedtypeleak

type opaqueTag uint

const (
	tagA opaqueTag = 1 << iota
	tagB
	tagC
)

func MethodNotAllowedHandler(allowed ...opaqueTag) {}

type level int

const (
	LevelDebug level = iota
	LevelInfo
	LevelWarn
)

func SetLevel(l level) {}

type token uint64

func NewToken() token  { return 0 }
func UseToken(t token) {}

type tier int

var DefaultTier tier = 7

func PromoteTo(t tier) {}

type band uint8

func GetVisitor() func() band { return nil }
func PromoteBand(b band)      {}
