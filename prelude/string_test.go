package lisette

import "testing"

func TestRuneAtASCII(t *testing.T) {
	s := "hello"
	want := []rune{'h', 'e', 'l', 'l', 'o'}
	for i, w := range want {
		if got := RuneAt(s, i); got != w {
			t.Errorf("RuneAt(%q, %d) = %q, want %q", s, i, got, w)
		}
	}
}

func TestRuneAtMultibyte(t *testing.T) {
	s := "héllo"
	want := []rune{'h', 'é', 'l', 'l', 'o'}
	for i, w := range want {
		if got := RuneAt(s, i); got != w {
			t.Errorf("RuneAt(%q, %d) = %q, want %q", s, i, got, w)
		}
	}
}

func TestRuneAtInvalidUTF8(t *testing.T) {
	s := "h\xc3"
	if got := RuneAt(s, 1); got != '�' {
		t.Errorf("expected RuneError for invalid byte, got %q", got)
	}
}

func TestRuneAtPanics(t *testing.T) {
	assertPanic(t, "rune index out of range", func() { RuneAt("hello", 5) })
	assertPanic(t, "rune index out of range", func() { RuneAt("hello", -1) })
	assertPanic(t, "rune index out of range", func() { RuneAt("", 0) })
}

func TestSubstring(t *testing.T) {
	cases := []struct {
		s          string
		start, end int
		want       string
	}{
		{"hello", 0, 5, "hello"},
		{"hello", 1, 4, "ell"},
		{"hello", 0, 0, ""},
		{"hello", 5, 5, ""},
		{"hello", 2, 2, ""},
		{"héllo", 0, 2, "hé"},
		{"héllo", 1, 4, "éll"},
		{"héllo", 0, 5, "héllo"},
		{"", 0, 0, ""},
	}
	for _, c := range cases {
		got := Substring(c.s, c.start, c.end)
		if got != c.want {
			t.Errorf("Substring(%q, %d, %d) = %q, want %q", c.s, c.start, c.end, got, c.want)
		}
	}
}

func TestSubstringPanics(t *testing.T) {
	assertPanic(t, "substring index out of range", func() { Substring("hello", -1, 3) })
	assertPanic(t, "substring index out of range", func() { Substring("hello", 0, -1) })
	assertPanic(t, "substring: start > end", func() { Substring("hello", 3, 1) })
	assertPanic(t, "substring index out of range", func() { Substring("hello", 6, 6) })
	assertPanic(t, "substring index out of range", func() { Substring("hello", 0, 6) })
}

func TestSubstringFrom(t *testing.T) {
	cases := []struct {
		s     string
		start int
		want  string
	}{
		{"hello", 0, "hello"},
		{"hello", 2, "llo"},
		{"hello", 5, ""},
		{"héllo", 1, "éllo"},
		{"héllo", 5, ""},
	}
	for _, c := range cases {
		got := SubstringFrom(c.s, c.start)
		if got != c.want {
			t.Errorf("SubstringFrom(%q, %d) = %q, want %q", c.s, c.start, got, c.want)
		}
	}
}

func TestSubstringFromPanics(t *testing.T) {
	assertPanic(t, "substring index out of range", func() { SubstringFrom("hello", -1) })
	assertPanic(t, "substring index out of range", func() { SubstringFrom("hello", 6) })
}

func TestSubstringTo(t *testing.T) {
	cases := []struct {
		s    string
		end  int
		want string
	}{
		{"hello", 0, ""},
		{"hello", 3, "hel"},
		{"hello", 5, "hello"},
		{"héllo", 2, "hé"},
		{"héllo", 5, "héllo"},
	}
	for _, c := range cases {
		got := SubstringTo(c.s, c.end)
		if got != c.want {
			t.Errorf("SubstringTo(%q, %d) = %q, want %q", c.s, c.end, got, c.want)
		}
	}
}

func TestSubstringToPanics(t *testing.T) {
	assertPanic(t, "substring index out of range", func() { SubstringTo("hello", -1) })
	assertPanic(t, "substring index out of range", func() { SubstringTo("hello", 6) })
}

func TestRuneAtZeroAlloc(t *testing.T) {
	s := "héllo world!"
	allocs := testing.AllocsPerRun(100, func() {
		_ = RuneAt(s, 5)
	})
	if allocs != 0 {
		t.Fatalf("RuneAt: expected 0 allocs, got %v", allocs)
	}
}

func TestSubstringZeroAlloc(t *testing.T) {
	s := "héllo world!"
	allocs := testing.AllocsPerRun(100, func() {
		_ = Substring(s, 1, 4)
	})
	if allocs != 0 {
		t.Fatalf("Substring: expected 0 allocs, got %v", allocs)
	}
}

func TestSubstringFromZeroAlloc(t *testing.T) {
	s := "héllo world!"
	allocs := testing.AllocsPerRun(100, func() {
		_ = SubstringFrom(s, 5)
	})
	if allocs != 0 {
		t.Fatalf("SubstringFrom: expected 0 allocs, got %v", allocs)
	}
}

func TestSubstringToZeroAlloc(t *testing.T) {
	s := "héllo world!"
	allocs := testing.AllocsPerRun(100, func() {
		_ = SubstringTo(s, 5)
	})
	if allocs != 0 {
		t.Fatalf("SubstringTo: expected 0 allocs, got %v", allocs)
	}
}

func assertPanic(t *testing.T, want string, fn func()) {
	t.Helper()
	defer func() {
		r := recover()
		if r == nil {
			t.Fatal("expected panic")
		}
		msg, ok := r.(string)
		if !ok {
			t.Fatalf("expected string panic, got %T: %v", r, r)
		}
		if msg != want {
			t.Fatalf("expected panic %q, got %q", want, msg)
		}
	}()
	fn()
}
