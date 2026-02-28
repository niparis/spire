package status

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func Infer(projectRoot string, slug string) (string, error) {
	if exists, err := pathExists(filepath.Join(projectRoot, "archive", slug)); err != nil {
		return "", err
	} else if exists {
		return "Complete", nil
	}

	changesDir := filepath.Join(projectRoot, "changes", slug)
	specAudit := filepath.Join(projectRoot, "specs", "feature-"+slug+"-AUDIT.md")
	planFile := filepath.Join(changesDir, "PLAN.md")
	sessionFile := filepath.Join(changesDir, "SESSION.md")
	verificationFile := filepath.Join(changesDir, "VERIFICATION_REPORT.md")

	if exists, err := pathExists(verificationFile); err != nil {
		return "", err
	} else if exists {
		return "Awaiting PR", nil
	}

	if exists, err := pathExists(sessionFile); err != nil {
		return "", err
	} else if exists {
		progress, err := parseSessionProgress(sessionFile)
		if err != nil {
			return "", err
		}
		if progress == "" {
			return "In progress", nil
		}
		return fmt.Sprintf("In progress (%s)", progress), nil
	}

	if exists, err := pathExists(planFile); err != nil {
		return "", err
	} else if exists {
		return "Awaiting implementation", nil
	}

	if exists, err := pathExists(specAudit); err != nil {
		return "", err
	} else if exists {
		return "Awaiting planning", nil
	}

	return "Spec only", nil
}

func parseSessionProgress(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("open session file %q: %w", path, err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(line, "Status:") {
			return strings.TrimSpace(strings.TrimPrefix(line, "Status:")), nil
		}
		if strings.HasPrefix(line, "Overall:") {
			return strings.TrimSpace(strings.TrimPrefix(line, "Overall:")), nil
		}
	}

	if err := scanner.Err(); err != nil {
		return "", fmt.Errorf("scan session file %q: %w", path, err)
	}

	return "", nil
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
