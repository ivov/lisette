package lisette

import "testing"

func TestPartialOk(t *testing.T) {
	p := MakePartialOk[int, string](42)
	if !p.IsOk() {
		t.Fatal("expected Ok")
	}
	if p.IsErr() {
		t.Fatal("expected not Err")
	}
	if p.IsBoth() {
		t.Fatal("expected not Both")
	}
}

func TestPartialErr(t *testing.T) {
	p := MakePartialErr[int, string]("fail")
	if p.IsOk() {
		t.Fatal("expected not Ok")
	}
	if !p.IsErr() {
		t.Fatal("expected Err")
	}
	if p.IsBoth() {
		t.Fatal("expected not Both")
	}
}

func TestPartialBoth(t *testing.T) {
	p := MakePartialBoth[int, string](42, "eof")
	if p.IsOk() {
		t.Fatal("expected not Ok")
	}
	if p.IsErr() {
		t.Fatal("expected not Err")
	}
	if !p.IsBoth() {
		t.Fatal("expected Both")
	}
}

func TestPartialOkMethod(t *testing.T) {
	ok := MakePartialOk[int, string](42)
	err := MakePartialErr[int, string]("fail")
	both := MakePartialBoth[int, string](42, "eof")

	if ok.Ok().IsNone() {
		t.Fatal("Ok().Ok() should be Some")
	}
	if err.Ok().IsSome() {
		t.Fatal("Err().Ok() should be None")
	}
	if both.Ok().IsNone() {
		t.Fatal("Both().Ok() should be Some")
	}
}

func TestPartialErrMethod(t *testing.T) {
	ok := MakePartialOk[int, string](42)
	err := MakePartialErr[int, string]("fail")
	both := MakePartialBoth[int, string](42, "eof")

	if ok.Err().IsSome() {
		t.Fatal("Ok().Err() should be None")
	}
	if err.Err().IsNone() {
		t.Fatal("Err().Err() should be Some")
	}
	if both.Err().IsNone() {
		t.Fatal("Both().Err() should be Some")
	}
}

func TestPartialUnwrapOr(t *testing.T) {
	ok := MakePartialOk[int, string](42)
	err := MakePartialErr[int, string]("fail")
	both := MakePartialBoth[int, string](42, "eof")

	if ok.UnwrapOr(0) != 42 {
		t.Fatal("Ok.UnwrapOr should return value")
	}
	if err.UnwrapOr(0) != 0 {
		t.Fatal("Err.UnwrapOr should return default")
	}
	if both.UnwrapOr(0) != 42 {
		t.Fatal("Both.UnwrapOr should return value")
	}
}

func TestPartialUnwrapOrElse(t *testing.T) {
	err := MakePartialErr[int, string]("fail")
	result := err.UnwrapOrElse(func(e string) int { return len(e) })
	if result != 4 {
		t.Fatalf("expected 4, got %d", result)
	}

	ok := MakePartialOk[int, string](42)
	result = ok.UnwrapOrElse(func(e string) int { return 0 })
	if result != 42 {
		t.Fatal("Ok.UnwrapOrElse should return value")
	}
}

func TestPartialMap(t *testing.T) {
	ok := MakePartialOk[int, string](21)
	mapped := PartialMap(ok, func(v int) int { return v * 2 })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("Ok map should transform value")
	}

	both := MakePartialBoth[int, string](21, "eof")
	mapped = PartialMap(both, func(v int) int { return v * 2 })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("Both map should transform value")
	}
	if mapped.Err().IsNone() {
		t.Fatal("Both map should preserve error")
	}

	err := MakePartialErr[int, string]("fail")
	mapped = PartialMap(err, func(v int) int { return v * 2 })
	if mapped.Ok().IsSome() {
		t.Fatal("Err map should not produce value")
	}
}

func TestPartialMapErr(t *testing.T) {
	err := MakePartialErr[int, string]("fail")
	mapped := PartialMapErr(err, func(e string) string { return e + "!" })
	if mapped.Err().UnwrapOr("") != "fail!" {
		t.Fatal("Err map_err should transform error")
	}

	both := MakePartialBoth[int, string](42, "eof")
	mapped = PartialMapErr(both, func(e string) string { return e + "!" })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("Both map_err should preserve value")
	}
	if mapped.Err().UnwrapOr("") != "eof!" {
		t.Fatal("Both map_err should transform error")
	}

	ok := MakePartialOk[int, string](42)
	mapped = PartialMapErr(ok, func(e string) string { return e + "!" })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("Ok map_err should preserve value")
	}
}

func TestPartialString(t *testing.T) {
	ok := MakePartialOk[int, string](42)
	if ok.String() != "Ok(42)" {
		t.Fatalf("expected Ok(42), got %s", ok.String())
	}

	err := MakePartialErr[int, string]("fail")
	if err.String() != "Err(fail)" {
		t.Fatalf("expected Err(fail), got %s", err.String())
	}

	both := MakePartialBoth[int, string](42, "eof")
	if both.String() != "Both(42, eof)" {
		t.Fatalf("expected Both(42, eof), got %s", both.String())
	}
}
