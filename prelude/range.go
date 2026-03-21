package lisette

type Range[T any] struct {
	Start T
	End   T
}

type RangeInclusive[T any] struct {
	Start T
	End   T
}

type RangeFrom[T any] struct {
	Start T
}

type RangeTo[T any] struct {
	End T
}

type RangeToInclusive[T any] struct {
	End T
}
