package commands

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunInitHappyPathCreatesMethodologyAndRootProjection(t *testing.T) {
	source := createMethodologySource(t)
	projectRoot := t.TempDir()
	t.Setenv("SPIRE_METHODOLOGY_SOURCE", source)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileContains(t, filepath.Join(projectRoot, ".methodology", "skills", "spec-auditor.md"), "Spec")
	assertFileContains(t, filepath.Join(projectRoot, "AGENTS.md"), "Project")
	assertFileContains(t, filepath.Join(projectRoot, ".gitignore"), ".methodology/")
}

func TestRunInitAlreadyInitializedAborts(t *testing.T) {
	source := createMethodologySource(t)
	projectRoot := t.TempDir()
	t.Setenv("SPIRE_METHODOLOGY_SOURCE", source)

	if err := os.MkdirAll(filepath.Join(projectRoot, ".methodology"), 0o755); err != nil {
		t.Fatalf("mkdir .methodology: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}

	if !strings.Contains(stderr.String(), "Already initialized") {
		t.Fatalf("stderr: got %q", stderr.String())
	}
}

func TestRunInitDoesNotOverwriteExistingAgentsFile(t *testing.T) {
	source := createMethodologySource(t)
	projectRoot := t.TempDir()
	t.Setenv("SPIRE_METHODOLOGY_SOURCE", source)

	existing := "# existing\nkeep me\n"
	if err := os.WriteFile(filepath.Join(projectRoot, "AGENTS.md"), []byte(existing), 0o644); err != nil {
		t.Fatalf("write existing AGENTS.md: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	data, err := os.ReadFile(filepath.Join(projectRoot, "AGENTS.md"))
	if err != nil {
		t.Fatalf("read AGENTS.md: %v", err)
	}

	if string(data) != existing {
		t.Fatalf("AGENTS.md was overwritten: got %q", string(data))
	}
}

func TestRunInitGitignoreEntryAddedOnlyOnce(t *testing.T) {
	source := createMethodologySource(t)
	projectRoot := t.TempDir()
	t.Setenv("SPIRE_METHODOLOGY_SOURCE", source)

	if err := os.WriteFile(filepath.Join(projectRoot, ".gitignore"), []byte(".methodology/\n"), 0o644); err != nil {
		t.Fatalf("write .gitignore: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	data, err := os.ReadFile(filepath.Join(projectRoot, ".gitignore"))
	if err != nil {
		t.Fatalf("read .gitignore: %v", err)
	}

	count := strings.Count(string(data), ".methodology/")
	if count != 1 {
		t.Fatalf(".methodology entry count: got %d, want 1; contents=%q", count, string(data))
	}
}

func createMethodologySource(t *testing.T) string {
	t.Helper()

	root := t.TempDir()

	writeFile(t, filepath.Join(root, "skills", "spec-auditor.md"), "# Spec\n")
	writeFile(t, filepath.Join(root, "project_root", "local_agents.md"), "# Project\n")
	writeFile(t, filepath.Join(root, "project_root", "manifest.json"), `{
  "version": 1,
  "mappings": [
    {
      "source": "local_agents.md",
      "destination": "AGENTS.md",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    }
  ]
}`)

	return root
}

func writeFile(t *testing.T, path string, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("mkdir parent for %s: %v", path, err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write file %s: %v", path, err)
	}
}

func assertFileContains(t *testing.T, path string, want string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	if !strings.Contains(string(data), want) {
		t.Fatalf("file %s does not contain %q; got %q", path, want, string(data))
	}
}
