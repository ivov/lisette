package param_sanitization

// Lisette reserved keyword as param name
func UseAs(as int) int { return as }

// Lisette-only keywords as param names
func UseLoop(loop int) int { return loop }
func UseTask(task string) string { return task }
func UseRecover(recover bool) bool { return recover }

// Uppercase param name (Go convention) lowercased for Lisette
func UppercaseParam(Name string) string { return Name }
