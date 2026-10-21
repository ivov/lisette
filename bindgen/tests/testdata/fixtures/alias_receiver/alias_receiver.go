// Fixture: a method declared on an alias receiver binds to the aliased type.
package alias_receiver

type Counter struct {
	Total int
}

type Tally = Counter

func (t *Tally) Add(n int) { t.Total += n }

func (t Tally) Value() int { return t.Total }
