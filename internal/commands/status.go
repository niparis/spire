package commands

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"

	projectstatus "opencode-spire/internal/status"
)

var statusSpecPattern = regexp.MustCompile(`^feature-(\d+)-(.+)\.md$`)

func RunStatus(args []string, projectRoot string, stdout io.Writer, stderr io.Writer) int {
	features, err := listFeatures(projectRoot)
	if err != nil {
		fmt.Fprintf(stderr, "failed to list features: %v\n", err)
		return 1
	}

	if len(features) == 0 {
		fmt.Fprintln(stdout, "No features yet. Run: spire new")
		return 0
	}

	rows := make([]projectstatus.Row, 0, len(features))
	for _, feature := range features {
		state, err := projectstatus.Infer(projectRoot, feature.Slug)
		if err != nil {
			fmt.Fprintf(stderr, "failed to infer status for %s: %v\n", feature.Slug, err)
			return 1
		}

		rows = append(rows, projectstatus.Row{
			Number:  feature.Number,
			Feature: feature.Name,
			Status:  state,
		})
	}

	fmt.Fprint(stdout, projectstatus.RenderTable(rows))
	return 0
}

type featureEntry struct {
	Number string
	Name   string
	Slug   string
}

func listFeatures(projectRoot string) ([]featureEntry, error) {
	specsDir := filepath.Join(projectRoot, "specs")
	entries, err := os.ReadDir(specsDir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}

	features := make([]featureEntry, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		name := entry.Name()
		if name == "_template.md" {
			continue
		}
		if len(name) >= 9 && name[len(name)-9:] == "-AUDIT.md" {
			continue
		}

		match := statusSpecPattern.FindStringSubmatch(name)
		if len(match) != 3 {
			continue
		}

		number := match[1]
		featureName := match[2]
		features = append(features, featureEntry{
			Number: number,
			Name:   featureName,
			Slug:   number + "-" + featureName,
		})
	}

	sort.Slice(features, func(i, j int) bool {
		return features[i].Slug < features[j].Slug
	})

	return features, nil
}
