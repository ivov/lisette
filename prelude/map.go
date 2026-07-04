package lisette

func MapGet[K comparable, V any](m map[K]V, key K) Option[V] {
	val, ok := m[key]
	if ok {
		return Option[V]{Tag: OptionSome, SomeVal: val}
	}
	return Option[V]{Tag: OptionNone}
}

func MapFrom[K comparable, V any](pairs []Tuple2[K, V]) map[K]V {
	result := make(map[K]V, len(pairs))
	for _, pair := range pairs {
		result[pair.First] = pair.Second
	}
	return result
}

func MapCloneFunc[M ~map[K]V, K comparable, V any](m M, clone func(V) V) M {
	if m == nil {
		return nil
	}
	out := make(M, len(m))
	for k, v := range m {
		out[k] = clone(v)
	}
	return out
}
