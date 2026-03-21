package lisette_test

import (
	"fmt"

	lisette "github.com/ivov/lisette/prelude"
)

func ExampleOption() {
	some := lisette.MakeOptionSome(42)
	none := lisette.MakeOptionNone[int]()

	fmt.Println(some.IsSome())
	fmt.Println(none.IsNone())
	fmt.Println(some.UnwrapOr(0))
	fmt.Println(none.UnwrapOr(0))
	// Output:
	// true
	// true
	// 42
	// 0
}

func ExampleResult() {
	ok := lisette.MakeResultOk[int, string](42)
	err := lisette.MakeResultErr[int, string]("something went wrong")

	fmt.Println(ok.IsOk())
	fmt.Println(err.IsErr())
	fmt.Println(ok.UnwrapOr(0))
	fmt.Println(err.UnwrapOr(0))
	// Output:
	// true
	// true
	// 42
	// 0
}

func ExampleOptionMap() {
	opt := lisette.MakeOptionSome(21)
	doubled := lisette.OptionMap(opt, func(v int) int { return v * 2 })
	fmt.Println(doubled)
	// Output:
	// Some(42)
}

func Example_mapGet() {
	m := map[string]int{"alice": 100, "bob": 200}
	fmt.Println(lisette.MapGet(m, "alice"))
	fmt.Println(lisette.MapGet(m, "charlie"))
	// Output:
	// Some(100)
	// None
}
