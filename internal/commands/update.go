package commands

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"opencode-spire/internal/methodology"
	"opencode-spire/internal/scaffold"
)

func RunUpdate(args []string, projectRoot string, stdin io.Reader, interactive bool, stdout io.Writer, stderr io.Writer) int {
	methodologyPath := filepath.Join(projectRoot, ".methodology")
	info, err := os.Stat(methodologyPath)
	if err != nil {
		if os.IsNotExist(err) {
			fmt.Fprintln(stderr, "Run spire init first.")
			return 1
		}
		fmt.Fprintf(stderr, "failed to inspect .methodology: %v\n", err)
		return 1
	}
	if !info.IsDir() {
		fmt.Fprintln(stderr, ".methodology exists but is not a directory")
		return 1
	}

	dirtyFiles, err := methodology.DetectDirty(methodologyPath)
	if err != nil {
		fmt.Fprintf(stderr, "failed to inspect local methodology edits: %v\n", err)
		return 1
	}

	if len(dirtyFiles) > 0 {
		fmt.Fprintln(stderr, "warning: local edits detected in .methodology:")
		for _, file := range dirtyFiles {
			fmt.Fprintf(stderr, "- %s\n", file)
		}

		if !interactive {
			fmt.Fprintln(stderr, "non-interactive mode: stash or remove local edits first.")
			return 1
		}

		if !confirmProceed(stdin, stderr) {
			fmt.Fprintln(stderr, "stash or remove local edits first.")
			return 1
		}
	}

	metadata, err := methodology.ReadSourceMetadata(methodologyPath)
	if err != nil {
		fmt.Fprintf(stderr, "failed to read methodology source metadata: %v\n", err)
		return 1
	}

	source := methodology.DefaultSourceMetadata()
	if metadata != nil {
		source = *metadata
	}

	changedFiles, _, err := methodology.SyncAndReportChangesFromMetadata(methodologyPath, source)
	if err != nil {
		fmt.Fprintf(stderr, "failed to update methodology payload: %v\n", err)
		return 1
	}

	fmt.Fprintln(stdout, "updated .methodology")
	if len(changedFiles) == 0 {
		fmt.Fprintln(stdout, "no methodology file changes detected")
	} else {
		fmt.Fprintln(stdout, "changed files:")
		for _, file := range changedFiles {
			fmt.Fprintf(stdout, "- %s\n", file)
		}
	}

	if err := scaffold.ApplyProjectRootUpdateMappings(projectRoot, methodologyPath, changedFiles, stdout); err != nil {
		fmt.Fprintf(stderr, "failed to apply project root mappings: %v\n", err)
		return 1
	}

	return 0
}

func confirmProceed(stdin io.Reader, stderr io.Writer) bool {
	if stdin == nil {
		return false
	}

	fmt.Fprint(stderr, "Continue? [y/N]: ")
	reader := bufio.NewReader(stdin)
	line, err := reader.ReadString('\n')
	if err != nil && err != io.EOF {
		return false
	}

	answer := strings.ToLower(strings.TrimSpace(line))
	return answer == "y" || answer == "yes"
}
