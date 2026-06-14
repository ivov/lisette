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

type partitioned struct {
	common          map[string]string    // pkgPath -> shared content
	variants        map[string][]variant // pkgPath -> per-content variants (divergent only)
	intendedTargets map[string][]Target  // pkgPath -> targets where it is available
}

// partitionByTarget classifies packages as common (suffixless, byte-identical
// across every target where present) or divergent (one variant per content
// equivalence class). Header-only outputs are dropped — they signal a
// build-tag stub like `log/syslog` on windows. Lookup gates by
// `intendedTargets` before falling through, so a shared file is never
// returned for an unsupported target.
func partitionByTarget(captured map[Target]map[string]string, targets []Target) partitioned {
	hasRealContent := make(map[string]bool)
	for _, results := range captured {
		for pkgPath, content := range results {
			if !isHeaderOnly(content) {
				hasRealContent[pkgPath] = true
			}
		}
	}
	for _, results := range captured {
		for pkgPath, content := range results {
			if isHeaderOnly(content) && hasRealContent[pkgPath] {
				delete(results, pkgPath)
			}
		}
	}

	allPkgs := make(map[string]struct{})
	for _, results := range captured {
		for pkgPath := range results {
			allPkgs[pkgPath] = struct{}{}
		}
	}

	result := partitioned{
		common:          make(map[string]string),
		variants:        make(map[string][]variant),
		intendedTargets: make(map[string][]Target),
	}

	for pkgPath := range allPkgs {
		var presentTargets []Target
		var groups []*variant
		for _, target := range targets {
			content, ok := captured[target][pkgPath]
			if !ok {
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

		if len(presentTargets) < len(targets) {
			result.intendedTargets[pkgPath] = presentTargets
		}

		if len(groups) == 1 {
			result.common[pkgPath] = groups[0].content
			continue
		}

		variants := make([]variant, len(groups))
		for i, g := range groups {
			variants[i] = *g
		}
		result.variants[pkgPath] = variants
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
