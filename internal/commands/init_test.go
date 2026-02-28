package commands

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"opencode-spire/internal/methodology"
)

func TestRunInitHappyPathCreatesMethodologyAndRootProjection(t *testing.T) {
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)
	projectRoot := t.TempDir()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	assertFileContains(t, filepath.Join(projectRoot, ".methodology", "skills", "spec-auditor.md"), "Spec")
	assertFileContains(t, filepath.Join(projectRoot, "AGENTS.md"), "Project")
	assertFileContains(t, filepath.Join(projectRoot, "opencode.json"), "\"instructions\"")
	assertFileNotContains(t, filepath.Join(projectRoot, "opencode.json"), "\"agents\"")
	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "agents", "plan.json"), "FEATURE_PLANNER.md")
	assertFileContains(t, filepath.Join(projectRoot, ".opencode", "agents", "verifier.json"), "\"mode\": \"subagent\"")
	assertFileContains(t, filepath.Join(projectRoot, ".gitignore"), ".methodology/")
	assertFileContains(t, filepath.Join(projectRoot, ".methodology", ".spire-source.json"), "\"repository\": \"niparis/spire\"")
}

func TestRunInitAlreadyInitializedAborts(t *testing.T) {
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)
	projectRoot := t.TempDir()

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

func TestRunInitDoesNotOverwriteExistingProjectedFiles(t *testing.T) {
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)
	projectRoot := t.TempDir()

	existingAgents := "# existing\nkeep me\n"
	if err := os.WriteFile(filepath.Join(projectRoot, "AGENTS.md"), []byte(existingAgents), 0o644); err != nil {
		t.Fatalf("write existing AGENTS.md: %v", err)
	}

	existingOpenCode := "{\n  \"instructions\": [\n    \"AGENTS.md\"\n  ]\n}\n"
	if err := os.WriteFile(filepath.Join(projectRoot, "opencode.json"), []byte(existingOpenCode), 0o644); err != nil {
		t.Fatalf("write existing opencode.json: %v", err)
	}

	existingPlanAgent := "{\n  \"instructions\": [\n    \"AGENTS.md\"\n  ]\n}\n"
	if err := os.MkdirAll(filepath.Join(projectRoot, ".opencode", "agents"), 0o755); err != nil {
		t.Fatalf("mkdir .opencode/agents: %v", err)
	}
	if err := os.WriteFile(filepath.Join(projectRoot, ".opencode", "agents", "plan.json"), []byte(existingPlanAgent), 0o644); err != nil {
		t.Fatalf("write existing plan agent: %v", err)
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}

	agentsData, err := os.ReadFile(filepath.Join(projectRoot, "AGENTS.md"))
	if err != nil {
		t.Fatalf("read AGENTS.md: %v", err)
	}

	if string(agentsData) != existingAgents {
		t.Fatalf("AGENTS.md was overwritten: got %q", string(agentsData))
	}

	opencodeData, err := os.ReadFile(filepath.Join(projectRoot, "opencode.json"))
	if err != nil {
		t.Fatalf("read opencode.json: %v", err)
	}

	if string(opencodeData) != existingOpenCode {
		t.Fatalf("opencode.json was overwritten: got %q", string(opencodeData))
	}

	planData, err := os.ReadFile(filepath.Join(projectRoot, ".opencode", "agents", "plan.json"))
	if err != nil {
		t.Fatalf("read .opencode/agents/plan.json: %v", err)
	}

	if string(planData) != existingPlanAgent {
		t.Fatalf(".opencode/agents/plan.json was overwritten: got %q", string(planData))
	}
}

func TestRunInitGitignoreEntryAddedOnlyOnce(t *testing.T) {
	source := createMethodologySource(t)
	configureCanonicalSourceFromDir(t, source)
	projectRoot := t.TempDir()

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

func TestRunInitFailsWhenSourceDownloadFails(t *testing.T) {
	projectRoot := t.TempDir()

	restore := methodology.SetCanonicalSourceForTesting("niparis/spire", "main", "https://127.0.0.1:1/not-available.tar.gz")
	t.Cleanup(restore)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunInit(nil, projectRoot, &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "failed to initialize methodology payload") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func createMethodologySource(t *testing.T) string {
	t.Helper()

	root := t.TempDir()

	writeFile(t, filepath.Join(root, "skills", "spec-auditor.md"), "# Spec\n")
	writeFile(t, filepath.Join(root, "agents", "SPIRE.md"), "# SPIRE\n")
	writeFile(t, filepath.Join(root, "project_root", "local_agents.md"), "# Project\n")
	writeFile(t, filepath.Join(root, "project_root", "opencode.json"), "{\n  \"instructions\": [\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "plan.json"), "{\n  \"instructions\": [\n    \".methodology/agents/FEATURE_PLANNER.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "default.json"), "{\n  \"instructions\": [\n    \".methodology/agents/CODE.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "verifier.json"), "{\n  \"mode\": \"subagent\",\n  \"instructions\": [\n    \".methodology/agents/VERIFICATION.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "docs-writer.json"), "{\n  \"instructions\": [\n    \".methodology/agents/DOCS_WRITER.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "investigator.json"), "{\n  \"instructions\": [\n    \".methodology/agents/INVESTIGATOR.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", ".opencode", "agents", "productengineer.json"), "{\n  \"instructions\": [\n    \".methodology/agents/ARCHITECTURE.md\",\n    \".methodology/agents/SPIRE.md\",\n    \"AGENTS.md\",\n    \"specs/PRODUCT.md\"\n  ]\n}\n")
	writeFile(t, filepath.Join(root, "project_root", "manifest.json"), `{
  "version": 1,
  "mappings": [
    {
      "source": "local_agents.md",
      "destination": "AGENTS.md",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": "opencode.json",
      "destination": "opencode.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/plan.json",
      "destination": ".opencode/agents/plan.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/default.json",
      "destination": ".opencode/agents/default.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/verifier.json",
      "destination": ".opencode/agents/verifier.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/docs-writer.json",
      "destination": ".opencode/agents/docs-writer.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/investigator.json",
      "destination": ".opencode/agents/investigator.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    },
    {
      "source": ".opencode/agents/productengineer.json",
      "destination": ".opencode/agents/productengineer.json",
      "on_init": "if_missing",
      "on_update": "never_overwrite",
      "notify_if_source_changed": true
    }
  ]
}`)

	return root
}

func configureCanonicalSourceFromDir(t *testing.T, sourceDir string) {
	t.Helper()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/tarball.tar.gz" {
			http.NotFound(w, r)
			return
		}

		tarballData := buildMethodologyTarball(t, sourceDir)
		w.Header().Set("Content-Type", "application/gzip")
		_, _ = w.Write(tarballData)
	}))
	t.Cleanup(server.Close)

	restore := methodology.SetCanonicalSourceForTesting("niparis/spire", "main", server.URL+"/tarball.tar.gz")
	t.Cleanup(restore)
}

func buildMethodologyTarball(t *testing.T, sourceDir string) []byte {
	t.Helper()

	var output bytes.Buffer
	gzipWriter := gzip.NewWriter(&output)
	tarWriter := tar.NewWriter(gzipWriter)

	err := filepath.WalkDir(sourceDir, func(path string, d os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}

		rel, err := filepath.Rel(sourceDir, path)
		if err != nil {
			return err
		}
		if rel == "." {
			return nil
		}

		tarPath := filepath.ToSlash(filepath.Join("spire-main", "methodology", rel))
		if d.IsDir() {
			hdr := &tar.Header{Name: tarPath + "/", Typeflag: tar.TypeDir, Mode: 0o755}
			return tarWriter.WriteHeader(hdr)
		}

		info, err := d.Info()
		if err != nil {
			return err
		}

		hdr := &tar.Header{Name: tarPath, Typeflag: tar.TypeReg, Mode: int64(info.Mode().Perm()), Size: info.Size()}
		if err := tarWriter.WriteHeader(hdr); err != nil {
			return err
		}

		f, err := os.Open(path)
		if err != nil {
			return err
		}
		defer f.Close()

		if _, err := io.Copy(tarWriter, f); err != nil {
			return err
		}

		return nil
	})
	if err != nil {
		t.Fatalf("build tarball: %v", err)
	}

	if err := tarWriter.Close(); err != nil {
		t.Fatalf("close tar writer: %v", err)
	}
	if err := gzipWriter.Close(); err != nil {
		t.Fatalf("close gzip writer: %v", err)
	}

	return output.Bytes()
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

func assertFileNotContains(t *testing.T, path string, want string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	if strings.Contains(string(data), want) {
		t.Fatalf("file %s unexpectedly contains %q; got %q", path, want, string(data))
	}
}
