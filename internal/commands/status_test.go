package commands

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunStatusEmptyProject(t *testing.T) {
	projectRoot := t.TempDir()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunStatus(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, want 0", exitCode)
	}
	if got := strings.TrimSpace(stdout.String()); got != "No features yet. Run: spire new" {
		t.Fatalf("stdout: got %q", got)
	}
}

func TestRunStatusTableOutput(t *testing.T) {
	projectRoot := t.TempDir()
	writeStatusFixture(t, filepath.Join(projectRoot, "specs", "feature-001-alpha.md"), "x")
	writeStatusFixture(t, filepath.Join(projectRoot, "changes", "001-alpha", "SESSION.md"), "Overall: task 1/2\n")
	writeStatusFixture(t, filepath.Join(projectRoot, "specs", "feature-002-beta.md"), "x")
	writeStatusFixture(t, filepath.Join(projectRoot, "specs", "feature-002-beta-AUDIT.md"), "x")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunStatus(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	output := stdout.String()
	if !strings.Contains(output, "#") || !strings.Contains(output, "Feature") || !strings.Contains(output, "Status") {
		t.Fatalf("missing table header: %q", output)
	}
	if !strings.Contains(output, "001") || !strings.Contains(output, "alpha") || !strings.Contains(output, "In progress (task 1/2)") {
		t.Fatalf("missing first row: %q", output)
	}
	if !strings.Contains(output, "002") || !strings.Contains(output, "beta") || !strings.Contains(output, "Awaiting planning") {
		t.Fatalf("missing second row: %q", output)
	}
}

func writeStatusFixture(t *testing.T, path string, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("mkdir %s: %v", path, err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}
