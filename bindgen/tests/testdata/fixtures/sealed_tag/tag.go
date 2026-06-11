package sealed_tag

// Tagged's seal signature contains a struct tag, so its identity has quotes
// that must be escaped in the typedef.
type Tagged interface {
	Do() int
	tagged(struct {
		X int `json:"x"`
	})
}
