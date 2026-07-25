package cli

import "strings"

// variant groups targets that share byte-identical content for one package.
// The canonical target supplies the filename and is the lex-first target in
// the group per `targets` iteration order.
type variant struct {
	canonical Target
	targets   []Target
	content   string
}

type packagePartition struct {
	targets  []Target
	variants []variant
}

type partitioned map[string]packagePartition

func partitionByTarget(captured map[Target]map[string]string, targets []Target) partitioned {
	hasRealContent := make(map[string]bool)
	for _, results := range captured {
		for pkgPath, content := range results {
			if !isHeaderOnly(content) {
				hasRealContent[pkgPath] = true
			}
		}
	}
	allPkgs := make(map[string]struct{})
	for _, results := range captured {
		for pkgPath := range results {
			allPkgs[pkgPath] = struct{}{}
		}
	}

	result := make(partitioned, len(allPkgs))

	for pkgPath := range allPkgs {
		var presentTargets []Target
		var groups []*variant
		for _, target := range targets {
			content, ok := captured[target][pkgPath]
			if !ok || (isHeaderOnly(content) && hasRealContent[pkgPath]) {
				continue
			}
			presentTargets = append(presentTargets, target)

			placed := false
			for _, g := range groups {
				if g.content == content {
					g.targets = append(g.targets, target)
					placed = true
					break
				}
			}
			if !placed {
				groups = append(groups, &variant{
					canonical: target,
					targets:   []Target{target},
					content:   content,
				})
			}
		}

		if len(groups) == 0 {
			continue
		}

		variants := make([]variant, len(groups))
		for i, g := range groups {
			variants[i] = *g
		}
		result[pkgPath] = packagePartition{
			targets:  presentTargets,
			variants: variants,
		}
	}

	return result
}

func isHeaderOnly(content string) bool {
	for line := range strings.SplitSeq(content, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "//") {
			continue
		}
		return false
	}
	return true
}
