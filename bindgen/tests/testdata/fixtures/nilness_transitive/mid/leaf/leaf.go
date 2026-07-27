// Leaf: the nil sources, one constructor-named and one not.
package leaf

type Thing struct{ ID int }

func NewNil() *Thing { return nil }

func FetchNil() *Thing { return nil }
