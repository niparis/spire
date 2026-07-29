package scaffold

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestCleanupOpencodeRemovesStaleTrackedFiles(t *testing.T) {
	t.Parallel()

	projectRoot := t.TempDir()
	staleFile := filepath.Join(projectRoot, ".opencode", "agents", "stale.md")
	if err := os.MkdirAll(filepath.Dir(staleFile), 0o755); err != nil {
		t.Fatalf("mkdir parent: %v", err)
	}
	if err := os.WriteFile(staleFile, []byte("stale\n"), 0o644); err != nil {
		t.Fatalf("write stale file: %v", err)
	}

	oldProjections := map[string]bool{
		".opencode/agents/stale.md": true,
	}
	expected := map[string]bool{
		".opencode/agents/current.md": true,
	}

	var out bytes.Buffer
	if err := CleanupOpencode(projectRoot, oldProjections, expected, &out); err != nil {
		t.Fatalf("cleanup: %v", err)
	}

	if _, err := os.Stat(staleFile); !os.IsNotExist(err) {
		t.Fatal("stale file was not removed")
	}

	if !bytes.Contains(out.Bytes(), []byte("removed stale: .opencode/agents/stale.md")) {
		t.Fatalf("missing removal notice: %q", out.String())
	}
}

func TestCleanupOpencodePreservesUntrackedFiles(t *testing.T) {
	t.Parallel()

	projectRoot := t.TempDir()
	customFile := filepath.Join(projectRoot, ".opencode", "agents", "custom.md")
	if err := os.MkdirAll(filepath.Dir(customFile), 0o755); err != nil {
		t.Fatalf("mkdir parent: %v", err)
	}
	if err := os.WriteFile(customFile, []byte("custom\n"), 0o644); err != nil {
		t.Fatalf("write custom file: %v", err)
	}

	oldProjections := map[string]bool{}
	expected := map[string]bool{
		".opencode/agents/current.md": true,
	}

	var out bytes.Buffer
	if err := CleanupOpencode(projectRoot, oldProjections, expected, &out); err != nil {
		t.Fatalf("cleanup: %v", err)
	}

	data, err := os.ReadFile(customFile)
	if err != nil {
		t.Fatalf("read custom file: %v", err)
	}
	if string(data) != "custom\n" {
		t.Fatalf("custom file was modified: %q", string(data))
	}

	if bytes.Contains(out.Bytes(), []byte("removed stale")) {
		t.Fatalf("spurious removal notice: %q", out.String())
	}
}

func TestCleanupOpencodeRemovesStaleSkillDirectories(t *testing.T) {
	t.Parallel()

	projectRoot := t.TempDir()
	staleDir := filepath.Join(projectRoot, ".opencode", "skills", "spire-old")
	staleFile := filepath.Join(staleDir, "SKILL.md")
	if err := os.MkdirAll(staleDir, 0o755); err != nil {
		t.Fatalf("mkdir stale dir: %v", err)
	}
	if err := os.WriteFile(staleFile, []byte("old\n"), 0o644); err != nil {
		t.Fatalf("write stale skill: %v", err)
	}

	oldProjections := map[string]bool{
		".opencode/skills/spire-old/SKILL.md": true,
	}
	expected := map[string]bool{
		".opencode/skills/spire-new/SKILL.md": true,
	}

	var out bytes.Buffer
	if err := CleanupOpencode(projectRoot, oldProjections, expected, &out); err != nil {
		t.Fatalf("cleanup: %v", err)
	}

	if _, err := os.Stat(staleDir); !os.IsNotExist(err) {
		t.Fatal("stale skill directory was not removed")
	}

	if !bytes.Contains(out.Bytes(), []byte("removed stale: .opencode/skills/spire-old/SKILL.md")) {
		t.Fatalf("missing removal notice: %q", out.String())
	}
}

func TestGetExpectedAgentProjections(t *testing.T) {
	t.Parallel()

	manifest := ProjectRootManifest{
		Version: 1,
		Mappings: []ProjectRootRule{
			{Source: "a.md", Destination: "AGENTS.md"},
			{Source: "b.md", Destination: ".opencode/agents/spec.md"},
			{Source: "c.md", Destination: ".opencode/skills/test.md"},
			{Source: "d.md", Destination: "README.md"},
		},
	}

	projections := GetExpectedAgentProjections(manifest)

	if len(projections) != 2 {
		t.Fatalf("expected 2 opencode projections, got %d", len(projections))
	}

	for _, p := range projections {
		if !filepath.IsLocal(p) {
			t.Fatalf("projection must be relative: %q", p)
		}
		if !filepath.IsAbs(filepath.Join("/tmp", p)) {
			// Just checking it's a valid relative path
		}
	}
}
