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

func TestRunUpdateUnknownFlagAborts(t *testing.T) {
	projectRoot := t.TempDir()
	var stdout bytes.Buffer
	var stderr bytes.Buffer

	exitCode := RunUpdate([]string{"--unknown"}, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "unknown update flag: --unknown") {
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

	if !strings.Contains(stdout.String(), "notice: upstream project_root/local_agents.md changed; kept existing AGENTS.md (rerun with --force to overwrite)") {
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

func TestRunUpdateForceOverwritesProtectedRootMapping(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	localPath := filepath.Join(projectRoot, "opencode.json")
	if err := os.WriteFile(localPath, []byte("{\"local\":true}\n"), 0o644); err != nil {
		t.Fatalf("write local opencode.json: %v", err)
	}

	upstreamContent := "{\n  \"instructions\": [\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\",\n    \".methodology/agents/CODE.md\"\n  ]\n}\n"
	writeFile(t, filepath.Join(source, "project_root", "opencode.json"), upstreamContent)

	var firstStdout bytes.Buffer
	var firstStderr bytes.Buffer
	firstExitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &firstStdout, &firstStderr)
	if firstExitCode != 0 {
		t.Fatalf("first update exit code: got %d, stderr=%q", firstExitCode, firstStderr.String())
	}
	if !strings.Contains(firstStdout.String(), "notice: upstream project_root/opencode.json changed; kept existing opencode.json (rerun with --force to overwrite)") {
		t.Fatalf("missing protected notice: %q", firstStdout.String())
	}

	data, err := os.ReadFile(localPath)
	if err != nil {
		t.Fatalf("read local opencode.json: %v", err)
	}
	if string(data) != "{\"local\":true}\n" {
		t.Fatalf("opencode.json overwritten without force: %q", string(data))
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate([]string{"--force"}, projectRoot, strings.NewReader(""), false, &stdout, &stderr)
	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "force-updated: opencode.json") {
		t.Fatalf("missing force update notice: %q", stdout.String())
	}

	data, err = os.ReadFile(localPath)
	if err != nil {
		t.Fatalf("read local opencode.json after force: %v", err)
	}
	if string(data) != upstreamContent {
		t.Fatalf("opencode.json was not force-updated; got %q", string(data))
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

func TestRunUpdateCreatesSkills(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "skills", "spire-product-definition", "SKILL.md"), "Product Definition")
	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "skills", "spire-new-feature", "SKILL.md"), "New Feature")
	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "skills", "spire-grill-me", "SKILL.md"), "Grill Me")
	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "skills", "spire-architecture-definition", "SKILL.md"), "Architecture Definition")
}

func TestRunUpdateRemovesStaleAgent(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	stalePath := filepath.Join(projectRoot, ".opencode", "agents", "stale-agent.md")
	if err := os.WriteFile(stalePath, []byte("stale\n"), 0o644); err != nil {
		t.Fatalf("write stale agent: %v", err)
	}

	methodologyDir := filepath.Join(projectRoot, ".methodology")
	if err := methodology.WriteSyncStateProjections(methodologyDir, map[string]bool{
		".opencode/agents/stale-agent.md": true,
	}); err != nil {
		t.Fatalf("write sync state projections: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	if _, err := os.Stat(stalePath); !os.IsNotExist(err) {
		t.Fatalf("stale agent was not removed")
	}

	if !strings.Contains(stdout.String(), "removed stale: .opencode/agents/stale-agent.md") {
		t.Fatalf("missing stale removal notice: %q", stdout.String())
	}
}

func TestRunUpdatePreservesCustomFiles(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	customPath := filepath.Join(projectRoot, ".opencode", "agents", "custom-agent.md")
	customContent := "# custom\n"
	if err := os.WriteFile(customPath, []byte(customContent), 0o644); err != nil {
		t.Fatalf("write custom agent: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	data, err := os.ReadFile(customPath)
	if err != nil {
		t.Fatalf("read custom agent: %v", err)
	}
	if string(data) != customContent {
		t.Fatalf("custom agent was modified: %q", string(data))
	}

	if strings.Contains(stdout.String(), "removed stale") && strings.Contains(stdout.String(), "custom-agent") {
		t.Fatalf("custom agent incorrectly reported as stale: %q", stdout.String())
	}
}

func TestRunUpdateIdempotent(t *testing.T) {
	projectRoot := t.TempDir()
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)

	if code := RunInit(nil, projectRoot, &bytes.Buffer{}, &bytes.Buffer{}); code != 0 {
		t.Fatalf("init failed with code %d", code)
	}

	var firstStdout bytes.Buffer
	var firstStderr bytes.Buffer
	if exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &firstStdout, &firstStderr); exitCode != 0 {
		t.Fatalf("first update failed with code %d, stderr=%q", exitCode, firstStderr.String())
	}

	var secondStdout bytes.Buffer
	var secondStderr bytes.Buffer
	if exitCode := RunUpdate(nil, projectRoot, strings.NewReader(""), false, &secondStdout, &secondStderr); exitCode != 0 {
		t.Fatalf("second update failed with code %d, stderr=%q", exitCode, secondStderr.String())
	}

	if strings.Contains(secondStdout.String(), "removed stale") {
		t.Fatalf("second update spuriously reported removals: %q", secondStdout.String())
	}
}
