// Package testkit backs Lisette's test-only TestContext. It is imported only by
// generated *_test.go files, so `go build` never pulls testing into production.
package testkit

import "testing"

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
func (c TestContext) Run(name string, body func(TestContext)) bool {
	return c.t.Run(name, func(inner *testing.T) {
		body(TestContext{t: inner})
	})
}

// Parallel marks this test as eligible to run in parallel.
func (c TestContext) Parallel() {
	c.t.Parallel()
}
