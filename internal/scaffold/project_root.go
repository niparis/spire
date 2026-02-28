package scaffold

import (
	"fmt"
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
