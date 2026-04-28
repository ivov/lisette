package reflection_decode

// Pointer-out shape: json.Unmarshal-like.
func Unmarshal(data []byte, v interface{}) error { return nil }

// Key + pointer-out shape: viper.UnmarshalKey-like.
func UnmarshalKey(key string, v interface{}) error { return nil }

// Multiple interface{} params: each lifts to its own T_n.
func DecodeBoth(src interface{}, dst interface{}) error { return nil }

// Variadic interface{} is intentionally NOT lifted (sql.Rows.Scan-shape).
func ScanRow(dest ...interface{}) error { return nil }

// Method on a value receiver.
type Decoder struct{}

func (d Decoder) Decode(v interface{}) error { return nil }

// Method on a pointer receiver, plus a non-whitelisted method that should
// keep its `interface{}` param as `Unknown`.
type Codec struct{}

func (c *Codec) Unmarshal(data []byte, v interface{}) error { return nil }

func (c *Codec) Set(v interface{}) {}

// Non-whitelisted free function with `interface{}` — stays `Unknown`.
func Log(v interface{}) {}

// `error` interface must NOT be lifted, even when whitelisted.
func DecodeWithErr(v interface{}, err error) error { return err }

// Param typed as `any` (alias for `interface{}`) must also be lifted.
func DecodeAny(v any) error { return nil }
