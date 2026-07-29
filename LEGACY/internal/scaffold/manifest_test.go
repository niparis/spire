package scaffold

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestLoadProjectRootManifest_Valid(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	path := filepath.Join(dir, "manifest.json")

	manifestJSON := `{
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
	}`

	if err := os.WriteFile(path, []byte(manifestJSON), 0o600); err != nil {
		t.Fatalf("write manifest: %v", err)
	}

	manifest, err := LoadProjectRootManifest(path)
	if err != nil {
		t.Fatalf("load manifest: %v", err)
	}

	if manifest.Version != 1 {
		t.Fatalf("version: got %d, want 1", manifest.Version)
	}

	if len(manifest.Mappings) != 1 {
		t.Fatalf("mappings len: got %d, want 1", len(manifest.Mappings))
	}

	mapping := manifest.Mappings[0]
	if mapping.Source != "local_agents.md" {
		t.Fatalf("source: got %q", mapping.Source)
	}
	if mapping.Destination != "AGENTS.md" {
		t.Fatalf("destination: got %q", mapping.Destination)
	}
	if mapping.OnInit != PolicyIfMissing {
		t.Fatalf("on_init: got %q", mapping.OnInit)
	}
	if mapping.OnUpdate != PolicyNeverOverwrite {
		t.Fatalf("on_update: got %q", mapping.OnUpdate)
	}
}

func TestLoadProjectRootManifest_InvalidSchemaReturnsTypedError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	path := filepath.Join(dir, "manifest.json")

	manifestJSON := `{
	  "version": 1,
	  "mappings": [
	    {
	      "source": "local_agents.md",
	      "destination": 123,
	      "on_init": "if_missing",
	      "on_update": "never_overwrite"
	    }
	  ]
	}`

	if err := os.WriteFile(path, []byte(manifestJSON), 0o600); err != nil {
		t.Fatalf("write manifest: %v", err)
	}

	_, err := LoadProjectRootManifest(path)
	if err == nil {
		t.Fatal("expected error, got nil")
	}

	var typedErr *ManifestError
	if !errors.As(err, &typedErr) {
		t.Fatalf("expected ManifestError, got %T", err)
	}

	if typedErr.Kind != "schema" {
		t.Fatalf("kind: got %q, want schema", typedErr.Kind)
	}
}

func TestLoadProjectRootManifest_PathTraversalRejected(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	path := filepath.Join(dir, "manifest.json")

	manifestJSON := `{
	  "version": 1,
	  "mappings": [
	    {
	      "source": "local_agents.md",
	      "destination": "../AGENTS.md",
	      "on_init": "if_missing",
	      "on_update": "never_overwrite"
	    }
	  ]
	}`

	if err := os.WriteFile(path, []byte(manifestJSON), 0o600); err != nil {
		t.Fatalf("write manifest: %v", err)
	}

	_, err := LoadProjectRootManifest(path)
	if err == nil {
		t.Fatal("expected error, got nil")
	}

	var typedErr *ManifestError
	if !errors.As(err, &typedErr) {
		t.Fatalf("expected ManifestError, got %T", err)
	}

	if typedErr.Kind != "validation" {
		t.Fatalf("kind: got %q, want validation", typedErr.Kind)
	}
}

func TestLoadProjectRootManifest_UnknownPolicyRejected(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	path := filepath.Join(dir, "manifest.json")

	manifestJSON := `{
	  "version": 1,
	  "mappings": [
	    {
	      "source": "local_agents.md",
	      "destination": "AGENTS.md",
	      "on_init": "overwrite",
	      "on_update": "never_overwrite"
	    }
	  ]
	}`

	if err := os.WriteFile(path, []byte(manifestJSON), 0o600); err != nil {
		t.Fatalf("write manifest: %v", err)
	}

	_, err := LoadProjectRootManifest(path)
	if err == nil {
		t.Fatal("expected error, got nil")
	}

	var typedErr *ManifestError
	if !errors.As(err, &typedErr) {
		t.Fatalf("expected ManifestError, got %T", err)
	}

	if typedErr.Kind != "validation" {
		t.Fatalf("kind: got %q, want validation", typedErr.Kind)
	}
}
