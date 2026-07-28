// Fixture: constructor prefixes match at word boundaries only, so Withdraw
// and Makeshift are not constructors while WithTag and MakeAccount are. Map
// indexing keeps every body inconclusive, leaving the name heuristic to
// decide.
package nilness_word_boundary

type Account struct{ Balance int }

var ledger map[string]*Account

func Withdraw(name string) *Account { return ledger[name] }

func WithTag(name string) *Account { return ledger[name] }

func Makeshift(name string) *Account { return ledger[name] }

func MakeAccount(name string) *Account { return ledger[name] }
