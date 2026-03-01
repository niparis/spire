package scaffold

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

func EnsureGitignoreEntry(projectRoot string, entry string) error {
	gitignorePath := filepath.Join(projectRoot, ".gitignore")
	entry = strings.TrimSpace(entry)
	if entry == "" {
		return nil
	}

	existing := ""
	if data, err := os.ReadFile(gitignorePath); err == nil {
		existing = string(data)
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("read .gitignore: %w", err)
	}

	for _, line := range strings.Split(existing, "\n") {
		if strings.TrimSpace(line) == entry {
			return nil
		}
	}

	var builder strings.Builder
	builder.WriteString(existing)
	if existing != "" && !strings.HasSuffix(existing, "\n") {
		builder.WriteString("\n")
	}
	builder.WriteString(entry)
	builder.WriteString("\n")

	if err := os.WriteFile(gitignorePath, []byte(builder.String()), 0o644); err != nil {
		return fmt.Errorf("write .gitignore: %w", err)
	}

	return nil
}

func copyFile(source string, destination string) error {
	in, err := os.Open(source)
	if err != nil {
		return fmt.Errorf("open source file %q: %w", source, err)
	}
	defer in.Close()

	info, err := in.Stat()
	if err != nil {
		return fmt.Errorf("stat source file %q: %w", source, err)
	}

	if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
		return fmt.Errorf("create destination parent %q: %w", filepath.Dir(destination), err)
	}

	out, err := os.OpenFile(destination, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, info.Mode().Perm())
	if err != nil {
		return fmt.Errorf("open destination file %q: %w", destination, err)
	}
	defer out.Close()

	if _, err := io.Copy(out, in); err != nil {
		return fmt.Errorf("copy %q to %q: %w", source, destination, err)
	}

	return nil
}

func ApplyProjectRootInitMappings(projectRoot string, methodologyDir string, out io.Writer) error {
	manifestPath := filepath.Join(methodologyDir, "project_root", "manifest.json")
	manifest, err := LoadProjectRootManifest(manifestPath)
	if err != nil {
		return err
	}

	sourceRoot := methodologyDir
	actions, err := BuildProjectRootActions(manifest, sourceRoot, ModeInit)
	if err != nil {
		return err
	}

	for _, action := range actions {
		destination := filepath.Join(projectRoot, action.Destination)
		exists, err := pathExists(destination)
		if err != nil {
			return err
		}

		if exists {
			fmt.Fprintf(out, "skipped existing: %s\n", action.Destination)
			continue
		}

		if err := copyFile(action.Source, destination); err != nil {
			return err
		}

		fmt.Fprintf(out, "created: %s\n", action.Destination)
	}

	return nil
}

func ApplyProjectRootUpdateMappings(projectRoot string, methodologyDir string, changedMethodologyFiles []string, out io.Writer) error {
	manifestPath := filepath.Join(methodologyDir, "project_root", "manifest.json")
	manifest, err := LoadProjectRootManifest(manifestPath)
	if err != nil {
		return err
	}

	sourceRoot := methodologyDir
	actions, err := BuildProjectRootActions(manifest, sourceRoot, ModeUpdate)
	if err != nil {
		return err
	}

	changedSet := map[string]bool{}
	for _, path := range changedMethodologyFiles {
		changedSet[path] = true
	}

	for _, action := range actions {
		destination := filepath.Join(projectRoot, action.Destination)
		exists, err := pathExists(destination)
		if err != nil {
			return err
		}

		sourceRel, err := filepath.Rel(methodologyDir, action.Source)
		if err != nil {
			return fmt.Errorf("compute source relative path for %q: %w", action.Source, err)
		}
		sourceRel = filepath.ToSlash(sourceRel)

		if exists && action.Policy == PolicyNeverOverwrite {
			if isManagedOpencodeDestination(action.Destination) && changedSet[sourceRel] {
				if err := copyFile(action.Source, destination); err != nil {
					return err
				}
				fmt.Fprintf(out, "updated: %s\n", action.Destination)
				continue
			}

			if action.NotifyIfSourceChanged && changedSet[sourceRel] {
				fmt.Fprintf(out, "notice: upstream %s changed; kept existing %s\n", sourceRel, action.Destination)
			}
			continue
		}

		if exists && action.Policy == PolicyIfMissing {
			continue
		}

		if err := copyFile(action.Source, destination); err != nil {
			return err
		}

		if exists {
			fmt.Fprintf(out, "updated: %s\n", action.Destination)
		} else {
			fmt.Fprintf(out, "created: %s\n", action.Destination)
		}
	}

	return nil
}

func pathExists(path string) (bool, error) {
	_, err := os.Stat(path)
	if err == nil {
		return true, nil
	}
	if os.IsNotExist(err) {
		return false, nil
	}
	return false, fmt.Errorf("stat %q: %w", path, err)
}

func isManagedOpencodeDestination(destination string) bool {
	destination = filepath.ToSlash(destination)
	return strings.HasPrefix(destination, ".opencode/")
}
