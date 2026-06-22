// Package testkit backs Lisette's TestContext, kept separate so production builds never import testing.
package testkit

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"testing"
)

// TestContext wraps *testing.T. The field is unexported so Lisette code reaches
// the handle only through the methods below.
type TestContext struct {
	t *testing.T
}

// New wraps a *testing.T. Generated test wrappers call this to construct the
// handle passed to a #[test] function.
func New(t *testing.T) TestContext {
	return TestContext{t: t}
}

// Run runs body as a named subtest, re-wrapping the subtest's *testing.T.
func (c TestContext) Run(name string, body func(TestContext)) {
	c.t.Run(name, func(inner *testing.T) {
		body(TestContext{t: inner})
	})
}

// Parallel marks this test as eligible to run in parallel.
func (c TestContext) Parallel() {
	c.t.Parallel()
}

// Skip stops the test and reports the reason to lis.
func (c TestContext) Skip(reason string) {
	c.t.Attr("lisette-skip", hex.EncodeToString([]byte(reason)))
	c.t.SkipNow()
}

// Recover turns an uncaught panic into a Lisette failure anchored at the test, so it reports like an
// assertion instead of dumping a Go stack. Test wrappers defer this.
func Recover(t *testing.T, file int, lo, hi uint32) {
	if r := recover(); r != nil {
		Fail(t, file, lo, hi, "panic", fmt.Sprintf("panic: %v", r))
	}
}

// FailAssert reports an `assert` failure over the same channel as Fail.
func (c TestContext) FailAssert(file int, lo, hi uint32, kind, message string, operands ...Operand) {
	Fail(c.t, file, lo, hi, kind, message, operands...)
}

type Operand struct {
	Label string `json:"label"`
	Value string `json:"value"`
	Lo    uint32 `json:"lo,omitempty"`
	Hi    uint32 `json:"hi,omitempty"`
}

func ErrOperand(value any) Operand {
	return Operand{Label: "error", Value: fmt.Sprintf("%v", value)}
}

type failRecord struct {
	File     int       `json:"file"`
	Lo       uint32    `json:"lo"`
	Hi       uint32    `json:"hi"`
	Kind     string    `json:"kind"`
	Message  string    `json:"message"`
	Operands []Operand `json:"operands,omitempty"`
}

// failEnvelope frames one chunk; `lis` orders by I and concatenates D over 0..N.
type failEnvelope struct {
	I int    `json:"i"`
	N int    `json:"n"`
	D string `json:"d"`
}

// Fail reports a test failure to `lis` over the t.Attr channel. The payload is hex
// (not raw JSON) so a chunk boundary never splits a UTF-8 rune and the chunk needs no
// JSON escaping, keeping each attr line's length predictable.
func Fail(t *testing.T, file int, lo, hi uint32, kind, message string, operands ...Operand) {
	inner, err := json.Marshal(failRecord{file, lo, hi, kind, message, operands})
	if err != nil {
		t.Fatalf("lisette: failed to encode failure record: %v", err)
		return
	}
	payload := hex.EncodeToString(inner)

	// test2json drops a framing line over 4096 bytes; size the hex chunk so the whole
	// `=== ATTR  <test> lisette-fail <envelope>` line stays under it, with margin for
	// the envelope skeleton and the i/n digits.
	budget := max(4096-64-len(t.Name()), 256)
	chunks := (len(payload) + budget - 1) / budget
	if chunks == 0 {
		chunks = 1
	}
	for i := 0; i < chunks; i++ {
		start := i * budget
		end := min(start+budget, len(payload))
		env, err := json.Marshal(failEnvelope{I: i, N: chunks, D: payload[start:end]})
		if err != nil {
			t.Fatalf("lisette: failed to encode failure envelope: %v", err)
			return
		}
		t.Attr("lisette-fail", string(env))
	}
	t.FailNow()
}
