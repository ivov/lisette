package lisette

import "unicode/utf8"

// RuneAt returns the rune at rune-index i in s. Panics on out-of-range i.
func RuneAt(s string, i int) rune {
	if i < 0 {
		panic("rune index out of range")
	}
	runeCount := 0
	for byteIdx := 0; byteIdx < len(s); {
		r, size := utf8.DecodeRuneInString(s[byteIdx:])
		if runeCount == i {
			return r
		}
		byteIdx += size
		runeCount++
	}
	panic("rune index out of range")
}

// Substring returns s[start:end] in rune indices. Panics on bad bounds.
func Substring(s string, start, end int) string {
	if start < 0 || end < 0 {
		panic("substring index out of range")
	}
	if start > end {
		panic("substring: start > end")
	}
	return s[byteOffsetOrPanic(s, start):byteOffsetOrPanic(s, end)]
}

// SubstringFrom returns s[start:] in rune indices. Panics on bad bounds.
func SubstringFrom(s string, start int) string {
	if start < 0 {
		panic("substring index out of range")
	}
	return s[byteOffsetOrPanic(s, start):]
}

// SubstringTo returns s[:end] in rune indices. Panics on bad bounds.
func SubstringTo(s string, end int) string {
	if end < 0 {
		panic("substring index out of range")
	}
	return s[:byteOffsetOrPanic(s, end)]
}

func byteOffsetOrPanic(s string, i int) int {
	off, ok := byteOffsetAtRune(s, i)
	if !ok {
		panic("substring index out of range")
	}
	return off
}

// byteOffsetAtRune returns the byte offset corresponding to rune index i,
// where i is in [0, runeCount(s)]. ok is false if i exceeds that range.
func byteOffsetAtRune(s string, i int) (int, bool) {
	if i == 0 {
		// i == 0 is before any rune; loop only matches post-increment.
		return 0, true
	}
	runeCount := 0
	for byteIdx := 0; byteIdx < len(s); {
		if s[byteIdx] < utf8.RuneSelf {
			byteIdx++
		} else {
			_, size := utf8.DecodeRuneInString(s[byteIdx:])
			byteIdx += size
		}
		runeCount++
		if runeCount == i {
			return byteIdx, true
		}
	}
	return 0, false
}
