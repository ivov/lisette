package lisette

import "maps"

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

func MapClone[M ~map[K]V, K comparable, V any](m M) M {
	out := make(M, len(m))
	maps.Copy(out, m)
	return out
}

func MapCloneFunc[M ~map[K]V, K comparable, V any](m M, clone func(V) V) M {
	out := make(M, len(m))
	for k, v := range m {
		out[k] = clone(v)
	}
	return out
}
