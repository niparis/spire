package commands

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"opencode-spire/internal/methodology"
)

func TestRunUpdateWithoutMethodologyAborts(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), true, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "Run spire init first") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunUpdateCleanReportsChangedFiles(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	writeFile(t, filepath.Join(source, "skills", "spec-auditor.md"), "# Spec v2\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	if !strings.Contains(stdout.String(), "changed files:") {
		t.Fatalf("stdout missing changed header: %q", stdout.String())
	}
	if !strings.Contains(stdout.String(), "skills/spec-auditor.md") {
		t.Fatalf("stdout missing changed file: %q", stdout.String())
	}
}

func TestRunUpdateDirtyPromptsAndAbortsOnNo(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	writeFile(t, filepath.Join(projectRoot, ".methodology", "skills", "spec-auditor.md"), "local edit\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader("n\n"), true, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "warning: local edits detected") {
		t.Fatalf("stderr missing warning: %q", stderr.String())
	}
	if !strings.Contains(stderr.String(), "stash or remove local edits first") {
		t.Fatalf("stderr missing abort guidance: %q", stderr.String())
	}
}

func TestRunUpdateDirtyContinuesOnYes(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	writeFile(t, filepath.Join(projectRoot, ".methodology", "skills", "spec-auditor.md"), "local edit\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader("y\n"), true, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "updated .methodology") {
		t.Fatalf("stdout: %q", stdout.String())
	}
}

func TestRunUpdateDirtyNonInteractiveAborts(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	writeFile(t, filepath.Join(projectRoot, ".methodology", "skills", "spec-auditor.md"), "local edit\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader("y\n"), false, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "non-interactive mode") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunUpdateRootMappingNoticeWithoutOverwrite(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	if err := os.WriteFile(filepath.Join(projectRoot, "AGENTS.md"), []byte("custom local\n"), 0o644); err != nil {
		t.Fatalf("write AGENTS.md: %v", err)
	}

	writeFile(t, filepath.Join(source, "project_root", "local_agents.md"), "# Project changed\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	if !strings.Contains(stdout.String(), "notice: upstream project_root/local_agents.md changed; kept existing AGENTS.md") {
		t.Fatalf("missing notice: %q", stdout.String())
	}

	data, err := os.ReadFile(filepath.Join(projectRoot, "AGENTS.md"))
	if err != nil {
		t.Fatalf("read AGENTS.md: %v", err)
	}
	if string(data) != "custom local\n" {
		t.Fatalf("AGENTS.md overwritten: %q", string(data))
	}
}

func TestRunUpdateOpencodeMappingOverwritesWhenSourceChanged(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	localPath := filepath.Join(projectRoot, ".opencode", "agents", "productengineer.md")
	if err := os.WriteFile(localPath, []byte("local custom\n"), 0o644); err != nil {
		t.Fatalf("write local productengineer.md: %v", err)
	}

	upstreamPath := filepath.Join(source, "project_root", ".opencode", "agents", "productengineer.md")
	upstreamContent := "---\nmode: primary\n---\nupstream changed\n"
	if err := os.WriteFile(upstreamPath, []byte(upstreamContent), 0o644); err != nil {
		t.Fatalf("write upstream productengineer.md: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	if !strings.Contains(stdout.String(), "updated: .opencode/agents/productengineer.md") {
		t.Fatalf("missing update notice: %q", stdout.String())
	}

	data, err := os.ReadFile(localPath)
	if err != nil {
		t.Fatalf("read local productengineer.md: %v", err)
	}
	if string(data) != upstreamContent {
		t.Fatalf("productengineer.md was not updated; got %q", string(data))
	}
}

func TestRunUpdateUsesStoredMetadataOverCurrentDefaults(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	writeFile(t, filepath.Join(source, "skills", "spec-auditor.md"), "# Spec v2\n")

	restoreBad := methodology.SetCanonicalSourceForTesting("niparis/spire", "main", "https://127.0.0.1:1/not-used.tar.gz")
	t.Cleanup(restoreBad)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "skills/spec-auditor.md") {
		t.Fatalf("stdout missing changed file: %q", stdout.String())
	}
}

func TestRunUpdateFallsBackWhenMetadataMissing(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	if err := os.Remove(filepath.Join(projectRoot, ".methodology", ".spire-source.json")); err != nil {
		t.Fatalf("remove source metadata: %v", err)
	}

	writeFile(t, filepath.Join(source, "skills", "spec-auditor.md"), "# Spec v2\n")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "updated .methodology") {
		t.Fatalf("stdout: %q", stdout.String())
	}
}
