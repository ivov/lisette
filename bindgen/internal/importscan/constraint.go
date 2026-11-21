// Adapted from cmd/go/internal/imports/build.go in the Go source tree.
//
// Copyright 2011 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found at https://go.dev/LICENSE.

package importscan

import (
	"bytes"
	"errors"
	"go/build/constraint"
	"unicode"
)

var (
	slashSlash     = []byte("//")
	starSlash      = []byte("*/")
	slashStar      = []byte("/*")
	plusBuild      = []byte("+build")
	goBuildComment = []byte("//go:build")

	errMultipleGoBuild = errors.New("multiple //go:build comments")
)

// matchesAnyTags applies the build constraint rule of `go mod tidy`.
func matchesAnyTags(content []byte) bool {
	content, goBuild, err := parseFileHeader(content)
	if err != nil {
		return false
	}

	if goBuild != nil {
		expr, err := constraint.Parse(string(goBuild))
		if err != nil {
			return false
		}
		return evalAnyTags(expr, true)
	}

	shouldBuild := true
	rest := content
	for len(rest) > 0 {
		line := rest
		if i := bytes.IndexByte(line, '\n'); i >= 0 {
			line, rest = line[:i], rest[i+1:]
		} else {
			rest = rest[len(rest):]
		}
		line = bytes.TrimSpace(line)
		if !bytes.HasPrefix(line, slashSlash) || !bytes.Contains(line, plusBuild) {
			continue
		}
		text := string(line)
		if !constraint.IsPlusBuild(text) {
			continue
		}
		if expr, err := constraint.Parse(text); err == nil && !evalAnyTags(expr, true) {
			shouldBuild = false
		}
	}
	return shouldBuild
}

func isGoBuildComment(line []byte) bool {
	if !bytes.HasPrefix(line, goBuildComment) {
		return false
	}
	line = bytes.TrimSpace(line)
	rest := line[len(goBuildComment):]
	return len(rest) == 0 || len(bytes.TrimSpace(rest)) < len(rest)
}

func parseFileHeader(content []byte) (trimmed, goBuild []byte, err error) {
	end := 0
	rest := content
	ended := false
	inSlashStar := false

Lines:
	for len(rest) > 0 {
		line := rest
		if i := bytes.IndexByte(line, '\n'); i >= 0 {
			line, rest = line[:i], rest[i+1:]
		} else {
			rest = rest[len(rest):]
		}
		line = bytes.TrimSpace(line)
		if len(line) == 0 && !ended {
			end = len(content) - len(rest)
			continue Lines
		}
		if !bytes.HasPrefix(line, slashSlash) {
			ended = true
		}

		if !inSlashStar && isGoBuildComment(line) {
			if goBuild != nil {
				return nil, nil, errMultipleGoBuild
			}
			goBuild = line
		}

	Comments:
		for len(line) > 0 {
			if inSlashStar {
				if i := bytes.Index(line, starSlash); i >= 0 {
					inSlashStar = false
					line = bytes.TrimSpace(line[i+len(starSlash):])
					continue Comments
				}
				continue Lines
			}
			if bytes.HasPrefix(line, slashSlash) {
				continue Lines
			}
			if bytes.HasPrefix(line, slashStar) {
				inSlashStar = true
				line = bytes.TrimSpace(line[len(slashStar):])
				continue Comments
			}
			break Lines
		}
	}

	return content[:end], goBuild, nil
}

func evalAnyTags(expr constraint.Expr, prefer bool) bool {
	switch expr := expr.(type) {
	case *constraint.TagExpr:
		return matchAnyTag(expr.Tag, prefer)
	case *constraint.NotExpr:
		return !evalAnyTags(expr.X, !prefer)
	case *constraint.AndExpr:
		return evalAnyTags(expr.X, prefer) && evalAnyTags(expr.Y, prefer)
	case *constraint.OrExpr:
		return evalAnyTags(expr.X, prefer) || evalAnyTags(expr.Y, prefer)
	}
	return false
}

func matchAnyTag(name string, prefer bool) bool {
	for _, c := range name {
		if !unicode.IsLetter(c) && !unicode.IsDigit(c) && c != '_' && c != '.' {
			return false
		}
	}
	if name != "" && name != "ignore" {
		return prefer
	}
	return false
}
