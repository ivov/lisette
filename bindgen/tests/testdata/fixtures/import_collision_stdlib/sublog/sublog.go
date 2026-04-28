// Local package whose declared name (`log`) collides with stdlib `log`.
package log

// Entry is a third-party-shaped log record.
type Entry struct {
	Level   int
	Message string
}
