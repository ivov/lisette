package lisette

func SliceGet[T any](s []T, index int) Option[T] {
	if index < 0 || index >= len(s) {
		return Option[T]{Tag: OptionNone}
	}
	return Option[T]{Tag: OptionSome, SomeVal: s[index]}
}

func SliceFilter[T any](s []T, f func(T) bool) []T {
	var result []T
	for _, v := range s {
		if f(v) {
			result = append(result, v)
		}
	}
	return result
}

func SliceMap[T any, U any](s []T, f func(T) U) []U {
	result := make([]U, len(s))
	for i, v := range s {
		result[i] = f(v)
	}
	return result
}

func SliceFold[T any, U any](s []T, init U, f func(U, T) U) U {
	result := init
	for _, v := range s {
		result = f(result, v)
	}
	return result
}

func SliceAll[T any](s []T, f func(T) bool) bool {
	for _, v := range s {
		if !f(v) {
			return false
		}
	}
	return true
}

func SliceFind[T any](s []T, predicate func(T) bool) Option[T] {
	for _, v := range s {
		if predicate(v) {
			return Option[T]{Tag: OptionSome, SomeVal: v}
		}
	}
	return Option[T]{Tag: OptionNone}
}

func EnumeratedSliceFilter[T any](s []T, f func(Tuple2[int, T]) bool) []Tuple2[int, T] {
	var result []Tuple2[int, T]
	for i, v := range s {
		pair := Tuple2[int, T]{First: i, Second: v}
		if f(pair) {
			result = append(result, pair)
		}
	}
	return result
}

func EnumeratedSliceMap[T any, U any](s []T, f func(Tuple2[int, T]) U) []U {
	result := make([]U, len(s))
	for i, v := range s {
		result[i] = f(Tuple2[int, T]{First: i, Second: v})
	}
	return result
}

func EnumeratedSliceFold[T any, U any](s []T, init U, f func(U, Tuple2[int, T]) U) U {
	result := init
	for i, v := range s {
		result = f(result, Tuple2[int, T]{First: i, Second: v})
	}
	return result
}

func EnumeratedSliceFind[T any](s []T, f func(Tuple2[int, T]) bool) Option[Tuple2[int, T]] {
	for i, v := range s {
		pair := Tuple2[int, T]{First: i, Second: v}
		if f(pair) {
			return Option[Tuple2[int, T]]{Tag: OptionSome, SomeVal: pair}
		}
	}
	return Option[Tuple2[int, T]]{Tag: OptionNone}
}
