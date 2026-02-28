package commands

import (
	"fmt"
	"io"
	"os"
	"path/filepath"

	"opencode-spire/internal/methodology"
	"opencode-spire/internal/scaffold"
)

func RunInit(args []string, projectRoot string, stdout io.Writer, stderr io.Writer) int {
	methodologyPath := filepath.Join(projectRoot, ".methodology")
	if _, err := os.Stat(methodologyPath); err == nil {
		fmt.Fprintln(stderr, "Already initialized: .methodology exists")
		return 1
	} else if !os.IsNotExist(err) {
		fmt.Fprintf(stderr, "failed to inspect .methodology: %v\n", err)
		return 1
	}

	source, err := methodology.ResolveSource()
	if err != nil {
		fmt.Fprintf(stderr, "failed to resolve methodology source: %v\n", err)
		return 1
	}

	if _, err := methodology.SyncToProject(source, projectRoot); err != nil {
		fmt.Fprintf(stderr, "failed to initialize methodology payload: %v\n", err)
		return 1
	}

	if err := scaffold.EnsureGitignoreEntry(projectRoot, ".methodology/"); err != nil {
		fmt.Fprintf(stderr, "failed to update .gitignore: %v\n", err)
		return 1
	}

	if err := scaffold.ApplyProjectRootInitMappings(projectRoot, methodologyPath, stdout); err != nil {
		fmt.Fprintf(stderr, "failed to apply project root mappings: %v\n", err)
		return 1
	}

	fmt.Fprintln(stdout, "initialized .methodology")
	return 0
}
