// Declares the same package name as the parent fixture but lives at a different
// import path, exercising the self-name vs import-alias collision.
package prelude_alias_collision

type Widget struct {
	ID int
}
