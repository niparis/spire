package status

import (
	"os"
	"path/filepath"
	"testing"
)

func TestInferStatuses(t *testing.T) {
	projectRoot := t.TempDir()

	write(t, filepath.Join(projectRoot, "specs", "feature-001-spec-only.md"), "x")
	write(t, filepath.Join(projectRoot, "specs", "feature-002-awaiting-planning.md"), "x")
	write(t, filepath.Join(projectRoot, "specs", "feature-002-awaiting-planning-AUDIT.md"), "x")
	write(t, filepath.Join(projectRoot, "specs", "feature-003-awaiting-implementation.md"), "x")
	write(t, filepath.Join(projectRoot, "changes", "003-awaiting-implementation", "PLAN.md"), "x")
	write(t, filepath.Join(projectRoot, "specs", "feature-004-in-progress.md"), "x")
	write(t, filepath.Join(projectRoot, "changes", "004-in-progress", "SESSION.md"), "Overall: task 2/5\n")
	write(t, filepath.Join(projectRoot, "specs", "feature-005-awaiting-pr.md"), "x")
	write(t, filepath.Join(projectRoot, "changes", "005-awaiting-pr", "VERIFICATION_REPORT.md"), "x")
	write(t, filepath.Join(projectRoot, "specs", "feature-006-complete.md"), "x")
	if err := os.MkdirAll(filepath.Join(projectRoot, "archive", "006-complete"), 0o755); err != nil {
		t.Fatalf("mkdir archive: %v", err)
	}

	tests := []struct {
		slug string
		want string
	}{
		{slug: "001-spec-only", want: "Spec only"},
		{slug: "002-awaiting-planning", want: "Awaiting planning"},
		{slug: "003-awaiting-implementation", want: "Awaiting implementation"},
		{slug: "004-in-progress", want: "In progress (task 2/5)"},
		{slug: "005-awaiting-pr", want: "Awaiting PR"},
		{slug: "006-complete", want: "Complete"},
	}

	for _, tc := range tests {
		t.Run(tc.slug, func(t *testing.T) {
			got, err := Infer(projectRoot, tc.slug)
			if err != nil {
				t.Fatalf("Infer error: %v", err)
			}
			if got != tc.want {
				t.Fatalf("status: got %q, want %q", got, tc.want)
			}
		})
	}
}

func TestParseSessionStatusLine(t *testing.T) {
	projectRoot := t.TempDir()
	write(t, filepath.Join(projectRoot, "specs", "feature-001-x.md"), "x")
	write(t, filepath.Join(projectRoot, "changes", "001-x", "SESSION.md"), "Status: task 3/7\n")

	got, err := Infer(projectRoot, "001-x")
	if err != nil {
		t.Fatalf("Infer error: %v", err)
	}
	if got != "In progress (task 3/7)" {
		t.Fatalf("status: got %q", got)
	}
}

func write(t *testing.T, path string, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("mkdir %s: %v", path, err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}
