// Fixtures for the `allow_unused_value` config override, which annotates
// package-level fluent registration functions with `#[allow(unused_value)]`.
// Models the shape of beego's `web.Get/Post/...` (return a singleton handle
// for chaining but are idiomatically called for side effect).
package allow_unused_value

type HttpServer struct{ routes []string }

func Register(path string, h func()) *HttpServer { return &HttpServer{} }

func Mount(prefix string, h func()) *HttpServer { return &HttpServer{} }

func NotConfigured(path string, h func()) *HttpServer { return &HttpServer{} }

func VoidReturn(path string) {}
