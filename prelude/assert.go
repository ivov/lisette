package lisette

func AssertType[T any](value any) Option[T] {
	if v, ok := value.(T); ok {
		return Option[T]{Tag: OptionSome, SomeVal: v}
	}
	return Option[T]{Tag: OptionNone}
}
