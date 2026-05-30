package scaffold

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHumanInvokedSkillsCount(t *testing.T) {
	t.Parallel()

	if len(HumanInvokedSkills) != 4 {
		t.Fatalf("expected 4 human-invoked skills, got %d", len(HumanInvokedSkills))
	}
}

func TestHumanInvokedSkillsHaveSpirePrefix(t *testing.T) {
	t.Parallel()

	for _, skill := range HumanInvokedSkills {
		if !strings.Contains(skill.Destination, "/spire-") {
			t.Fatalf("skill destination missing spire- prefix: %q", skill.Destination)
		}
	}
}

func TestGetExpectedSkillProjections(t *testing.T) {
	t.Parallel()

	projections := GetExpectedSkillProjections()
	if len(projections) != len(HumanInvokedSkills) {
		t.Fatalf("expected %d projections, got %d", len(HumanInvokedSkills), len(projections))
	}

	for _, p := range projections {
		if !strings.HasSuffix(p, "/SKILL.md") {
			t.Fatalf("projection must end with /SKILL.md: %q", p)
		}
	}
}

func TestApplySkillProjections(t *testing.T) {
	t.Parallel()

	methodologyDir := t.TempDir()
	projectRoot := t.TempDir()

	writeSkillFile(t, filepath.Join(methodologyDir, "skills", "product-definition.md"), "# Product\n")
	writeSkillFile(t, filepath.Join(methodologyDir, "skills", "new-feature", "SKILL.md"), "# New Feature\n")
	writeSkillFile(t, filepath.Join(methodologyDir, "skills", "grill-me", "SKILL.md"), "# Grill\n")
	writeSkillFile(t, filepath.Join(methodologyDir, "skills", "architecture-definition.md"), "# Arch\n")

	var out bytes.Buffer
	if err := ApplySkillProjections(methodologyDir, projectRoot, &out); err != nil {
		t.Fatalf("apply skill projections: %v", err)
	}

	assertSkillExists(t, projectRoot, ".opencode/skills/spire-product-definition/SKILL.md", "# Product\n")
	assertSkillExists(t, projectRoot, ".opencode/skills/spire-new-feature/SKILL.md", "# New Feature\n")
	assertSkillExists(t, projectRoot, ".opencode/skills/spire-grill-me/SKILL.md", "# Grill\n")
	assertSkillExists(t, projectRoot, ".opencode/skills/spire-architecture-definition/SKILL.md", "# Arch\n")

	if !strings.Contains(out.String(), "created skill:") {
		t.Fatalf("expected 'created skill' in output, got: %q", out.String())
	}
}

func TestApplySkillProjectionsSkipsMissingSkills(t *testing.T) {
	t.Parallel()

	methodologyDir := t.TempDir()
	projectRoot := t.TempDir()

	writeSkillFile(t, filepath.Join(methodologyDir, "skills", "product-definition.md"), "# Product\n")

	var out bytes.Buffer
	if err := ApplySkillProjections(methodologyDir, projectRoot, &out); err != nil {
		t.Fatalf("apply skill projections: %v", err)
	}

	assertSkillExists(t, projectRoot, ".opencode/skills/spire-product-definition/SKILL.md", "# Product\n")

	_, err := os.Stat(filepath.Join(projectRoot, ".opencode", "skills", "spire-new-feature", "SKILL.md"))
	if !os.IsNotExist(err) {
		t.Fatalf("expected missing skill to be skipped")
	}
}

func TestBuildExpectedProjections(t *testing.T) {
	t.Parallel()

	manifest := ProjectRootManifest{
		Version: 1,
		Mappings: []ProjectRootRule{
			{Source: "a.md", Destination: "AGENTS.md", OnInit: PolicyIfMissing, OnUpdate: PolicyNeverOverwrite},
			{Source: "b.md", Destination: ".opencode/agents/spec-auditor.md", OnInit: PolicyIfMissing, OnUpdate: PolicyNeverOverwrite},
			{Source: "c.md", Destination: ".opencode/skills/test.md", OnInit: PolicyIfMissing, OnUpdate: PolicyNeverOverwrite},
		},
	}

	expected := BuildExpectedProjections(manifest)

	if expected["AGENTS.md"] {
		t.Fatal("project-root files should not be in expected projections")
	}

	if !expected[".opencode/agents/spec-auditor.md"] {
		t.Fatal("missing expected agent projection")
	}

	if !expected[".opencode/skills/test.md"] {
		t.Fatal("missing expected skill projection from manifest")
	}

	if !expected[".opencode/skills/spire-product-definition/SKILL.md"] {
		t.Fatal("missing expected human-invoked skill projection")
	}

	if !expected[".opencode/skills/spire-new-feature/SKILL.md"] {
		t.Fatal("missing expected human-invoked skill projection")
	}
}

func writeSkillFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("mkdir parent for %s: %v", path, err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write file %s: %v", path, err)
	}
}

func assertSkillExists(t *testing.T, projectRoot, relPath, wantContent string) {
	t.Helper()
	path := filepath.Join(projectRoot, filepath.FromSlash(relPath))
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	if string(data) != wantContent {
		t.Fatalf("file %s content mismatch: got %q, want %q", path, string(data), wantContent)
	}
}
