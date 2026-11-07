// Fixture: a constructor name does not decide nilness. Map indexing keeps
// every body inconclusive, so each result is Option, unless a
// non_nilable_return pin states the contract.
package nilness_constructor_name

type Account struct{ Balance int }

var ledger map[string]*Account

func Withdraw(name string) *Account { return ledger[name] }

func WithTag(name string) *Account { return ledger[name] }

func MakeAccount(name string) *Account { return ledger[name] }

func NewAccount(name string) *Account { return ledger[name] }
