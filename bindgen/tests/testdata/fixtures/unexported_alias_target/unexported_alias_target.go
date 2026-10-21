// Fixture: an exported alias names an unexported struct, so references render the alias.
package unexported_alias_target

type options struct {
	Retries int
	verbose bool
}

func (o *options) Verbose() bool { return o.verbose }

type Options = options

func Defaults() Options { return options{Retries: 3} }

func Apply(o *Options) int { return o.Retries }

type Client struct {
	Settings Options
}
