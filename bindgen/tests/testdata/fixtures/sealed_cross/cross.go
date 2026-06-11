package sealed_cross

import "go/ast"

// MyDecl embeds a cross-package sealed interface; its seal identity must name
// go/ast (the declaring package), not this one.
type MyDecl interface {
	ast.Decl
}
