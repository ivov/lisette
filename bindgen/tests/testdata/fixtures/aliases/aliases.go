package aliases

// Type aliases (using =)

type AliasString = string
type AliasIntSlice = []int
type AliasStringMap = map[string]int

// Function using type aliases
func TakeAlias(s AliasString) AliasIntSlice {
	return nil
}

// Function returning alias
func GetMap() AliasStringMap {
	return nil
}

// Type definitions (new distinct types)

type MyInt int
type MyString string
type ID uint64

// Type definition over slice - hits convertType default path
type IntList []int

// Type definition over map
type StringMap map[string]int

// Function type aliases

// Handler is a function that handles requests.
type Handler func(request string) (response string, err error)

// Callback is a simple callback function.
type Callback func()

// Processor processes data.
type Processor func(data []byte) []byte

// Alias-to-array — return position must lower to Slice + #[go(array_return)]
// after peeling the alias.
type Digest = [32]byte

func ComputeDigest() Digest {
	return Digest{}
}
