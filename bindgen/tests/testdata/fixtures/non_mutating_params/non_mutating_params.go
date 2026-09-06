package non_mutating_params

var inspect = func(data []byte) { _ = len(data) }

// InferredView keeps conservative permissions without an override.
func InferredView(data []byte) []byte {
	inspect(data)
	return data
}

// ReadOnlyView overrides an inferred write using the sanitized parameter name.
func ReadOnlyView(self []byte) []byte {
	inspect(self)
	return self
}

// Peek overrides the dst name heuristic and keeps the returned view read-only.
func Peek(dst []byte) []byte { return dst[:1] }

// Copy keeps the destination writable while the source and its view are read-only.
func Copy(dst, src []byte) []byte {
	inspect(src)
	copy(dst, src)
	return src
}

type Reader struct{}

// View exercises the Type.Method override key.
func (r *Reader) View(data []byte) []byte {
	inspect(data)
	return data
}
