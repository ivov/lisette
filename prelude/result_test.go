package lisette

import "testing"

func TestResultOk(t *testing.T) {
	res := MakeResultOk[int, string](42)
	if !res.IsOk() {
		t.Fatal("expected Ok")
	}
	if res.IsErr() {
		t.Fatal("expected not Err")
	}
}

func TestResultErr(t *testing.T) {
	res := MakeResultErr[int, string]("fail")
	if res.IsOk() {
		t.Fatal("expected not Ok")
	}
	if !res.IsErr() {
		t.Fatal("expected Err")
	}
}

func TestResultOkMethod(t *testing.T) {
	ok := MakeResultOk[int, string](42)
	err := MakeResultErr[int, string]("fail")
	if ok.Ok().IsNone() {
		t.Fatal("expected Some from Ok()")
	}
	if err.Ok().IsSome() {
		t.Fatal("expected None from Ok()")
	}
}

func TestResultErrMethod(t *testing.T) {
	ok := MakeResultOk[int, string](42)
	err := MakeResultErr[int, string]("fail")
	if ok.Err().IsSome() {
		t.Fatal("expected None from Err()")
	}
	if err.Err().IsNone() {
		t.Fatal("expected Some from Err()")
	}
}

func TestResultUnwrapOr(t *testing.T) {
	ok := MakeResultOk[int, string](42)
	err := MakeResultErr[int, string]("fail")
	if ok.UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}
	if err.UnwrapOr(0) != 0 {
		t.Fatal("expected 0")
	}
}

func TestResultUnwrapOrElse(t *testing.T) {
	err := MakeResultErr[int, string]("fail")
	if err.UnwrapOrElse(func(e string) int { return len(e) }) != 4 {
		t.Fatal("expected 4")
	}
}

func TestResultString(t *testing.T) {
	ok := MakeResultOk[int, string](42)
	err := MakeResultErr[int, string]("fail")
	if ok.String() != "Ok(42)" {
		t.Fatalf("expected Ok(42), got %s", ok.String())
	}
	if err.String() != "Err(fail)" {
		t.Fatalf("expected Err(fail), got %s", err.String())
	}
}

func TestResultMap(t *testing.T) {
	ok := MakeResultOk[int, string](21)
	mapped := ResultMap(ok, func(v int) int { return v * 2 })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}
}

func TestResultMapErr(t *testing.T) {
	err := MakeResultErr[int, string]("fail")
	mapped := ResultMapErr(err, func(e string) int { return len(e) })
	if mapped.Err().UnwrapOr(0) != 4 {
		t.Fatal("expected 4")
	}
}

func TestResultAndThen(t *testing.T) {
	ok := MakeResultOk[int, string](42)
	chained := ResultAndThen(ok, func(v int) Result[string, string] {
		return MakeResultOk[string, string]("hello")
	})
	if chained.UnwrapOr("") != "hello" {
		t.Fatal("expected hello")
	}
}

func TestResultOrElse(t *testing.T) {
	err := MakeResultErr[int, string]("fail")
	recovered := ResultOrElse(err, func(e string) Result[int, int] {
		return MakeResultOk[int, int](99)
	})
	if recovered.UnwrapOr(0) != 99 {
		t.Fatal("expected 99")
	}
}
