package scaffold

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

type CopyPolicy string

const (
	PolicyIfMissing      CopyPolicy = "if_missing"
	PolicyNeverOverwrite CopyPolicy = "never_overwrite"
)

type ProjectRootManifest struct {
	Version  int               `json:"version"`
	Mappings []ProjectRootRule `json:"mappings"`
}

type ProjectRootRule struct {
	Source                string     `json:"source"`
	Destination           string     `json:"destination"`
	OnInit                CopyPolicy `json:"on_init"`
	OnUpdate              CopyPolicy `json:"on_update"`
	NotifyIfSourceChanged bool       `json:"notify_if_source_changed"`
}

type ManifestError struct {
	Kind  string
	Field string
	Err   error
}

func (e *ManifestError) Error() string {
	if e.Field == "" {
		return fmt.Sprintf("manifest %s: %v", e.Kind, e.Err)
	}
	return fmt.Sprintf("manifest %s (%s): %v", e.Kind, e.Field, e.Err)
}

func (e *ManifestError) Unwrap() error {
	return e.Err
}

func LoadProjectRootManifest(filePath string) (ProjectRootManifest, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return ProjectRootManifest{}, &ManifestError{Kind: "io", Err: err}
	}

	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()

	var manifest ProjectRootManifest
	if err := decoder.Decode(&manifest); err != nil {
		return ProjectRootManifest{}, &ManifestError{Kind: "schema", Err: err}
	}

	if err := ValidateProjectRootManifest(manifest); err != nil {
		return ProjectRootManifest{}, err
	}

	return manifest, nil
}

func ValidateProjectRootManifest(manifest ProjectRootManifest) error {
	if manifest.Version != 1 {
		return &ManifestError{Kind: "validation", Field: "version", Err: fmt.Errorf("unsupported version %d", manifest.Version)}
	}

	if len(manifest.Mappings) == 0 {
		return &ManifestError{Kind: "validation", Field: "mappings", Err: errors.New("must contain at least one mapping")}
	}

	for i, mapping := range manifest.Mappings {
		fieldPrefix := fmt.Sprintf("mappings[%d]", i)

		if err := validateManifestPath(mapping.Source); err != nil {
			return &ManifestError{Kind: "validation", Field: fieldPrefix + ".source", Err: err}
		}

		if err := validateManifestPath(mapping.Destination); err != nil {
			return &ManifestError{Kind: "validation", Field: fieldPrefix + ".destination", Err: err}
		}

		if err := validatePolicy(mapping.OnInit); err != nil {
			return &ManifestError{Kind: "validation", Field: fieldPrefix + ".on_init", Err: err}
		}

		if err := validatePolicy(mapping.OnUpdate); err != nil {
			return &ManifestError{Kind: "validation", Field: fieldPrefix + ".on_update", Err: err}
		}
	}

	return nil
}

func validatePolicy(policy CopyPolicy) error {
	switch policy {
	case PolicyIfMissing, PolicyNeverOverwrite:
		return nil
	default:
		return fmt.Errorf("unknown policy %q", policy)
	}
}

func validateManifestPath(value string) error {
	if strings.TrimSpace(value) == "" {
		return errors.New("path cannot be empty")
	}

	if filepath.IsAbs(value) {
		return fmt.Errorf("path must be relative: %q", value)
	}

	cleaned := filepath.Clean(value)
	if cleaned == "." {
		return fmt.Errorf("path cannot resolve to current directory: %q", value)
	}

	if strings.HasPrefix(cleaned, "..") {
		return fmt.Errorf("path traversal is not allowed: %q", value)
	}

	return nil
}
