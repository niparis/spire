package scaffold

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

type ProjectionMode string

const (
	ModeInit   ProjectionMode = "init"
	ModeUpdate ProjectionMode = "update"
)

type ProjectionAction struct {
	Source                string
	Destination           string
	Policy                CopyPolicy
	NotifyIfSourceChanged bool
}

func BuildProjectRootActions(manifest ProjectRootManifest, sourceRoot string, mode ProjectionMode) ([]ProjectionAction, error) {
	if err := ValidateProjectRootManifest(manifest); err != nil {
		return nil, err
	}

	sourceRoot = filepath.Clean(sourceRoot)
	actions := make([]ProjectionAction, 0, len(manifest.Mappings))

	for _, mapping := range manifest.Mappings {
		policy, err := policyForMode(mapping, mode)
		if err != nil {
			return nil, err
		}

		source := filepath.Join(sourceRoot, mapping.Source)
		cleanSource := filepath.Clean(source)
		if !isPathWithin(sourceRoot, cleanSource) {
			return nil, fmt.Errorf("source escapes root: %s", mapping.Source)
		}

		actions = append(actions, ProjectionAction{
			Source:                cleanSource,
			Destination:           filepath.Clean(mapping.Destination),
			Policy:                policy,
			NotifyIfSourceChanged: mapping.NotifyIfSourceChanged,
		})
	}

	return actions, nil
}

func policyForMode(mapping ProjectRootRule, mode ProjectionMode) (CopyPolicy, error) {
	switch mode {
	case ModeInit:
		return mapping.OnInit, nil
	case ModeUpdate:
		return mapping.OnUpdate, nil
	default:
		return "", fmt.Errorf("unknown projection mode %q", mode)
	}
}

func isPathWithin(root string, target string) bool {
	rel, err := filepath.Rel(root, target)
	if err != nil {
		return false
	}
	return rel == "." || (!strings.HasPrefix(rel, "..") && rel != "")
}

// GetExpectedAgentProjections returns manifest destinations that live under
// .opencode/ (agents and skills mapped from the manifest itself).
func GetExpectedAgentProjections(manifest ProjectRootManifest) []string {
	var projections []string
	for _, mapping := range manifest.Mappings {
		dest := filepath.ToSlash(mapping.Destination)
		if strings.HasPrefix(dest, ".opencode/") {
			projections = append(projections, dest)
		}
	}
	return projections
}

// BuildExpectedProjections returns the union of manifest-derived .opencode/
// projections and human-invoked skill projections.
func BuildExpectedProjections(manifest ProjectRootManifest) map[string]bool {
	expected := make(map[string]bool)
	for _, path := range GetExpectedAgentProjections(manifest) {
		expected[path] = true
	}
	for _, path := range GetExpectedSkillProjections() {
		expected[path] = true
	}
	return expected
}

// CleanupOpencode removes tracked projections that are no longer in the
// expected set. Files that were never tracked (user-created) are left untouched.
func CleanupOpencode(projectRoot string, oldProjections, expectedProjections map[string]bool, out io.Writer) error {
	for path := range oldProjections {
		if !expectedProjections[path] {
			fullPath := filepath.Join(projectRoot, path)

			// For skills, remove the entire containing directory since skills
			// are self-contained. For agents, remove just the file.
			if strings.HasPrefix(path, ".opencode/skills/") {
				dir := filepath.Dir(fullPath)
				if err := os.RemoveAll(dir); err != nil && !os.IsNotExist(err) {
					return fmt.Errorf("remove stale skill directory %q: %w", dir, err)
				}
			} else {
				if err := os.RemoveAll(fullPath); err != nil && !os.IsNotExist(err) {
					return fmt.Errorf("remove stale projection %q: %w", path, err)
				}
			}
			fmt.Fprintf(out, "removed stale: %s\n", path)
		}
	}
	return nil
}
