package commands

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunNewFirstFeatureUses001(t *testing.T) {
	projectRoot := setupNewCommandProject(t)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("User Auth\n"), &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileExists(t, filepath.Join(projectRoot, "specs", "feature-001-user-auth.md"))
	assertFileExists(t, filepath.Join(projectRoot, "changes", "001-user-auth", "SESSION.md"))
}

func TestRunNewUsesMaxPlusOneNumbering(t *testing.T) {
	projectRoot := setupNewCommandProject(t)
	writeFile(t, filepath.Join(projectRoot, "specs", "feature-001-one.md"), "x")
	writeFile(t, filepath.Join(projectRoot, "specs", "feature-002-two.md"), "x")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("Three\n"), &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileExists(t, filepath.Join(projectRoot, "specs", "feature-003-three.md"))
}

func TestRunNewGapUsesMaxPlusOne(t *testing.T) {
	projectRoot := setupNewCommandProject(t)
	writeFile(t, filepath.Join(projectRoot, "specs", "feature-001-one.md"), "x")
	writeFile(t, filepath.Join(projectRoot, "specs", "feature-003-three.md"), "x")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("Four\n"), &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileExists(t, filepath.Join(projectRoot, "specs", "feature-004-four.md"))
}

func TestRunNewSanitizesName(t *testing.T) {
	projectRoot := setupNewCommandProject(t)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("My Fancy FEATURE\n"), &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileExists(t, filepath.Join(projectRoot, "specs", "feature-001-my-fancy-feature.md"))
}

func TestRunNewEmptyNameAborts(t *testing.T) {
	projectRoot := setupNewCommandProject(t)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("   \n"), &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "Name required") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunNewDuplicateSpecAborts(t *testing.T) {
	projectRoot := setupNewCommandProject(t)
	writeFile(t, filepath.Join(projectRoot, "specs", "feature-001-user-auth.md"), "existing")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("User Auth\n"), &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "Spec already exists") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunNewTemplateSubstitution(t *testing.T) {
	projectRoot := setupNewCommandProject(t)
	writeFile(t, filepath.Join(projectRoot, "specs", "_template.md"), "Feature [Feature Name] number [NUMBER] date YYYY-MM-DD\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunNew(nil, projectRoot, strings.NewReader("User Auth\n"), &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	specPath := filepath.Join(projectRoot, "specs", "feature-001-user-auth.md")
	content := string(mustReadFile(t, specPath))
	if !strings.Contains(content, "Feature user-auth") {
		t.Fatalf("spec missing feature substitution: %q", content)
	}
	if !strings.Contains(content, "number 001") {
		t.Fatalf("spec missing number substitution: %q", content)
	}
	if strings.Contains(content, "YYYY-MM-DD") {
		t.Fatalf("spec still contains date placeholder: %q", content)
	}

	sessionPath := filepath.Join(projectRoot, "changes", "001-user-auth", "SESSION.md")
	session := string(mustReadFile(t, sessionPath))
	if strings.Contains(session, "YYYY-MM-DD") {
		t.Fatalf("session still contains date placeholder: %q", session)
	}
}

func setupNewCommandProject(t *testing.T) string {
	t.Helper()
	projectRoot := t.TempDir()

	writeFile(t, filepath.Join(projectRoot, ".methodology", "templates", "spec-template.md"), "# Spec: [Feature Name]\nDate: YYYY-MM-DD\nN: [NUMBER]\n")
	writeFile(t, filepath.Join(projectRoot, ".methodology", "templates", "session-template.md"), "# Session Log: [Feature Name]\nLast updated: YYYY-MM-DD\n")

	return projectRoot
}

func assertFileExists(t *testing.T, path string) {
	t.Helper()
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected file %s to exist: %v", path, err)
	}
}

func mustReadFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return data
}
