package lisette

import (
	"database/sql"
	"database/sql/driver"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

var (
	_ driver.Valuer = Option[int]{}
	_ sql.Scanner   = (*Option[int])(nil)
)

func TestOptionSome(t *testing.T) {
	opt := MakeOptionSome(42)
	if !opt.IsSome() {
		t.Fatal("expected Some")
	}
	if opt.IsNone() {
		t.Fatal("expected not None")
	}
}

func TestOptionNone(t *testing.T) {
	opt := MakeOptionNone[int]()
	if opt.IsSome() {
		t.Fatal("expected not Some")
	}
	if !opt.IsNone() {
		t.Fatal("expected None")
	}
}

func TestOptionUnwrapOr(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()
	if some.UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}
	if none.UnwrapOr(0) != 0 {
		t.Fatal("expected 0")
	}
}

func TestOptionUnwrapOrElse(t *testing.T) {
	none := MakeOptionNone[int]()
	if none.UnwrapOrElse(func() int { return 99 }) != 99 {
		t.Fatal("expected 99")
	}
}

func TestOptionFilter(t *testing.T) {
	some := MakeOptionSome(42)
	kept := some.Filter(func(v int) bool { return v > 10 })
	dropped := some.Filter(func(v int) bool { return v > 100 })
	if kept.IsNone() {
		t.Fatal("expected Some after filter")
	}
	if dropped.IsSome() {
		t.Fatal("expected None after filter")
	}
}

func TestOptionTake(t *testing.T) {
	opt := MakeOptionSome(42)
	taken := opt.Take()
	if taken.IsNone() {
		t.Fatal("expected Some from Take")
	}
	if opt.IsSome() {
		t.Fatal("expected None after Take")
	}
}

func TestOptionOrElse(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()
	if some.OrElse(func() Option[int] { return MakeOptionSome(99) }).UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}
	if none.OrElse(func() Option[int] { return MakeOptionSome(99) }).UnwrapOr(0) != 99 {
		t.Fatal("expected 99")
	}
}

func TestOptionString(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()
	if some.String() != "Some(42)" {
		t.Fatalf("expected Some(42), got %s", some.String())
	}
	if none.String() != "None" {
		t.Fatalf("expected None, got %s", none.String())
	}
}

func TestOptionIsZero(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()
	if some.IsZero() {
		t.Fatal("Some should not be zero")
	}
	if !none.IsZero() {
		t.Fatal("None should be zero")
	}
}

func TestOptionJSON(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()

	someBytes, err := json.Marshal(some)
	if err != nil {
		t.Fatal(err)
	}
	if string(someBytes) != "42" {
		t.Fatalf("expected 42, got %s", someBytes)
	}

	noneBytes, err := json.Marshal(none)
	if err != nil {
		t.Fatal(err)
	}
	if string(noneBytes) != "null" {
		t.Fatalf("expected null, got %s", noneBytes)
	}

	var unmarshaled Option[int]
	if err := json.Unmarshal([]byte("42"), &unmarshaled); err != nil {
		t.Fatal(err)
	}
	if unmarshaled.UnwrapOr(0) != 42 {
		t.Fatal("expected 42 after unmarshal")
	}

	var unmarshaledNone Option[int]
	if err := json.Unmarshal([]byte("null"), &unmarshaledNone); err != nil {
		t.Fatal(err)
	}
	if !unmarshaledNone.IsNone() {
		t.Fatal("expected None after unmarshal null")
	}
}

func TestOptionMap(t *testing.T) {
	some := MakeOptionSome(21)
	mapped := OptionMap(some, func(v int) int { return v * 2 })
	if mapped.UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}

	none := MakeOptionNone[int]()
	mappedNone := OptionMap(none, func(v int) int { return v * 2 })
	if mappedNone.IsSome() {
		t.Fatal("expected None")
	}
}

func TestOptionAndThen(t *testing.T) {
	some := MakeOptionSome(42)
	chained := OptionAndThen(some, func(v int) Option[string] {
		return MakeOptionSome("hello")
	})
	if chained.UnwrapOr("") != "hello" {
		t.Fatal("expected hello")
	}
}

func TestOptionOkOr(t *testing.T) {
	some := MakeOptionSome(42)
	none := MakeOptionNone[int]()
	if OptionOkOr(some, "err").IsErr() {
		t.Fatal("expected Ok")
	}
	if OptionOkOr(none, "err").IsOk() {
		t.Fatal("expected Err")
	}
}

func TestOptionFlatten(t *testing.T) {
	nested := MakeOptionSome(MakeOptionSome(42))
	flat := OptionFlatten(nested)
	if flat.UnwrapOr(0) != 42 {
		t.Fatal("expected 42")
	}
}

func TestOptionZip(t *testing.T) {
	a := MakeOptionSome(1)
	b := MakeOptionSome("hello")
	zipped := OptionZip(a, b)
	if zipped.IsNone() {
		t.Fatal("expected Some")
	}
}

func TestOptionValueSome(t *testing.T) {
	cases := []struct {
		name string
		opt  driver.Valuer
		want any
	}{
		{"int", MakeOptionSome(42), int64(42)},
		{"string", MakeOptionSome("hello"), "hello"},
		{"bool", MakeOptionSome(true), true},
		{"float32", MakeOptionSome(float32(1.5)), float64(1.5)},
		{"uint", MakeOptionSome(uint(7)), int64(7)},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, err := c.opt.Value()
			if err != nil {
				t.Fatal(err)
			}
			if got != c.want {
				t.Fatalf("got %v (%T), want %v (%T)", got, got, c.want, c.want)
			}
		})
	}
}

func TestOptionValueNone(t *testing.T) {
	got, err := MakeOptionNone[int]().Value()
	if err != nil {
		t.Fatal(err)
	}
	if got != nil {
		t.Fatalf("None should Value to nil, got %v", got)
	}
}

func TestOptionValueTime(t *testing.T) {
	tm := time.Date(2026, 4, 26, 12, 0, 0, 0, time.UTC)
	got, err := MakeOptionSome(tm).Value()
	if err != nil {
		t.Fatal(err)
	}
	if !got.(time.Time).Equal(tm) {
		t.Fatalf("got %v, want %v", got, tm)
	}
}

func TestOptionValueBytes(t *testing.T) {
	got, err := MakeOptionSome([]byte{1, 2, 3}).Value()
	if err != nil {
		t.Fatal(err)
	}
	if string(got.([]byte)) != "\x01\x02\x03" {
		t.Fatalf("got %v", got)
	}
}

func TestOptionValueUnsupported(t *testing.T) {
	type weird struct{ X int }
	_, err := MakeOptionSome(weird{1}).Value()
	if err == nil {
		t.Fatal("expected error for unsupported type")
	}
	if !strings.Contains(err.Error(), "weird") {
		t.Fatalf("error should mention type, got %v", err)
	}
}

func TestOptionValueUint64HighBit(t *testing.T) {
	_, err := MakeOptionSome(uint64(1) << 63).Value()
	if err == nil {
		t.Fatal("expected error for uint64 with high bit set")
	}
}

func TestOptionScanNil(t *testing.T) {
	opt := MakeOptionSome(99)
	if err := opt.Scan(nil); err != nil {
		t.Fatal(err)
	}
	if !opt.IsNone() {
		t.Fatalf("expected None, got %v", opt)
	}
}

func TestOptionScanInt(t *testing.T) {
	var opt Option[int]
	if err := opt.Scan(int64(42)); err != nil {
		t.Fatal(err)
	}
	if !opt.IsSome() || opt.SomeVal != 42 {
		t.Fatalf("got %v, want Some(42)", opt)
	}
}

type optionScanFailer struct{}

func (f *optionScanFailer) Scan(src any) error { return errors.New("scan boom") }

func TestOptionScanCustomScannerError(t *testing.T) {
	var opt Option[optionScanFailer]
	if err := opt.Scan("x"); err == nil || !strings.Contains(err.Error(), "scan boom") {
		t.Fatalf("expected scan boom error, got %v", err)
	}
}

func TestOptionScanNilUnsupportedT(t *testing.T) {
	type weird struct{ X int }
	var opt Option[weird]
	if err := opt.Scan(nil); err != nil {
		t.Fatal(err)
	}
	if !opt.IsNone() {
		t.Fatal("nil should scan to None")
	}
}
