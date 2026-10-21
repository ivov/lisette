// Fixture: an embedded unexported type keeps its own name, even when an exported alias names it.
package embedded_alias_target

type options struct {
	Retries int
	verbose bool
}

func (o *options) Verbose() bool { return o.verbose }

type Options = options

// Server embeds the type, so Go names the field `options`.
type Server struct {
	options
	Port int
}
