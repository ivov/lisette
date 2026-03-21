package lisette

import "fmt"

type Tuple2[T any, U any] struct {
	First  T
	Second U
}

func MakeTuple2[T any, U any](first T, second U) Tuple2[T, U] {
	return Tuple2[T, U]{First: first, Second: second}
}

func (t Tuple2[T, U]) String() string {
	return fmt.Sprintf("(%v, %v)", t.First, t.Second)
}

type Tuple3[T any, U any, V any] struct {
	First  T
	Second U
	Third  V
}

func MakeTuple3[T any, U any, V any](first T, second U, third V) Tuple3[T, U, V] {
	return Tuple3[T, U, V]{First: first, Second: second, Third: third}
}

func (t Tuple3[T, U, V]) String() string {
	return fmt.Sprintf("(%v, %v, %v)", t.First, t.Second, t.Third)
}

type Tuple4[T any, U any, V any, W any] struct {
	First  T
	Second U
	Third  V
	Fourth W
}

func MakeTuple4[T any, U any, V any, W any](first T, second U, third V, fourth W) Tuple4[T, U, V, W] {
	return Tuple4[T, U, V, W]{First: first, Second: second, Third: third, Fourth: fourth}
}

func (t Tuple4[T, U, V, W]) String() string {
	return fmt.Sprintf("(%v, %v, %v, %v)", t.First, t.Second, t.Third, t.Fourth)
}

type Tuple5[T any, U any, V any, W any, X any] struct {
	First  T
	Second U
	Third  V
	Fourth W
	Fifth  X
}

func MakeTuple5[T any, U any, V any, W any, X any](first T, second U, third V, fourth W, fifth X) Tuple5[T, U, V, W, X] {
	return Tuple5[T, U, V, W, X]{First: first, Second: second, Third: third, Fourth: fourth, Fifth: fifth}
}

func (t Tuple5[T, U, V, W, X]) String() string {
	return fmt.Sprintf("(%v, %v, %v, %v, %v)", t.First, t.Second, t.Third, t.Fourth, t.Fifth)
}
