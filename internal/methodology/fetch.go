package methodology

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	defaultSourceRepository = "niparis/spire"
	defaultSourceRef        = "main"
	sourceMetadataFilename  = ".spire-source.json"
)

var (
	httpClient          = &http.Client{Timeout: 30 * time.Second}
	canonicalRepository = defaultSourceRepository
	canonicalRef        = defaultSourceRef
	canonicalTarballURL = ""
)

type SourceMetadata struct {
	Repository string `json:"repository"`
	Ref        string `json:"ref"`
	TarballURL string `json:"tarball_url"`
	FetchedAt  string `json:"fetched_at"`
}

func DefaultSourceMetadata() SourceMetadata {
	return SourceMetadata{
		Repository: canonicalRepository,
		Ref:        canonicalRef,
		TarballURL: tarballURLFor(canonicalRepository, canonicalRef),
	}
}

func SyncCanonicalToProject(projectRoot string) (string, SourceMetadata, error) {
	meta := DefaultSourceMetadata()
	return syncSourceToProject(meta, projectRoot)
}

func SyncAndReportChangesFromMetadata(localDir string, metadata SourceMetadata) ([]string, SourceMetadata, error) {
	meta, err := normalizeSourceMetadata(metadata)
	if err != nil {
		return nil, SourceMetadata{}, err
	}

	sourceDir, cleanup, err := materializeSource(meta)
	if err != nil {
		return nil, SourceMetadata{}, err
	}
	defer cleanup()

	changedFiles, err := SyncAndReportChanges(localDir, sourceDir)
	if err != nil {
		return nil, SourceMetadata{}, err
	}

	meta.FetchedAt = time.Now().UTC().Format(time.RFC3339)
	if err := writeSourceMetadata(localDir, meta); err != nil {
		return nil, SourceMetadata{}, err
	}

	return changedFiles, meta, nil
}

func ReadSourceMetadata(localDir string) (*SourceMetadata, error) {
	path := sourceMetadataPath(localDir)
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("read source metadata: %w", err)
	}

	var metadata SourceMetadata
	if err := json.Unmarshal(data, &metadata); err != nil {
		return nil, fmt.Errorf("parse source metadata: %w", err)
	}

	normalized, err := normalizeSourceMetadata(metadata)
	if err != nil {
		return nil, fmt.Errorf("invalid source metadata: %w", err)
	}

	return &normalized, nil
}

func SetCanonicalSourceForTesting(repository string, ref string, tarballURL string) func() {
	prevRepo := canonicalRepository
	prevRef := canonicalRef
	prevTarballURL := canonicalTarballURL

	canonicalRepository = repository
	canonicalRef = ref
	canonicalTarballURL = strings.TrimSpace(tarballURL)

	return func() {
		canonicalRepository = prevRepo
		canonicalRef = prevRef
		canonicalTarballURL = prevTarballURL
	}
}

func sourceMetadataPath(localDir string) string {
	return filepath.Join(localDir, sourceMetadataFilename)
}

func writeSourceMetadata(localDir string, metadata SourceMetadata) error {
	data, err := json.MarshalIndent(metadata, "", "  ")
	if err != nil {
		return fmt.Errorf("serialize source metadata: %w", err)
	}

	if err := os.WriteFile(sourceMetadataPath(localDir), data, 0o644); err != nil {
		return fmt.Errorf("write source metadata: %w", err)
	}

	return nil
}

func syncSourceToProject(metadata SourceMetadata, projectRoot string) (string, SourceMetadata, error) {
	meta, err := normalizeSourceMetadata(metadata)
	if err != nil {
		return "", SourceMetadata{}, err
	}

	sourceDir, cleanup, err := materializeSource(meta)
	if err != nil {
		return "", SourceMetadata{}, err
	}
	defer cleanup()

	destination, err := SyncToProject(sourceDir, projectRoot)
	if err != nil {
		return "", SourceMetadata{}, err
	}

	meta.FetchedAt = time.Now().UTC().Format(time.RFC3339)
	if err := writeSourceMetadata(destination, meta); err != nil {
		return "", SourceMetadata{}, err
	}

	return destination, meta, nil
}

func normalizeSourceMetadata(metadata SourceMetadata) (SourceMetadata, error) {
	repository := strings.TrimSpace(metadata.Repository)
	if repository == "" {
		repository = canonicalRepository
	}

	ref := strings.TrimSpace(metadata.Ref)
	if ref == "" {
		ref = canonicalRef
	}

	tarballURL := strings.TrimSpace(metadata.TarballURL)
	if tarballURL == "" {
		tarballURL = tarballURLFor(repository, ref)
	}

	return SourceMetadata{
		Repository: repository,
		Ref:        ref,
		TarballURL: tarballURL,
		FetchedAt:  metadata.FetchedAt,
	}, nil
}

func tarballURLFor(repository string, ref string) string {
	if strings.TrimSpace(canonicalTarballURL) != "" {
		return canonicalTarballURL
	}

	if strings.HasPrefix(ref, "v") {
		return fmt.Sprintf("https://github.com/%s/archive/refs/tags/%s.tar.gz", repository, ref)
	}
	return fmt.Sprintf("https://github.com/%s/archive/refs/heads/%s.tar.gz", repository, ref)
}

func materializeSource(metadata SourceMetadata) (string, func(), error) {
	resp, err := httpClient.Get(metadata.TarballURL)
	if err != nil {
		return "", nil, fmt.Errorf("download methodology tarball: %w", err)
	}

	if resp.StatusCode != http.StatusOK {
		resp.Body.Close()
		return "", nil, fmt.Errorf("download methodology tarball: unexpected status %s", resp.Status)
	}

	tempDir, err := os.MkdirTemp("", "spire-methodology-*")
	if err != nil {
		resp.Body.Close()
		return "", nil, fmt.Errorf("create temp dir: %w", err)
	}

	cleanup := func() {
		resp.Body.Close()
		_ = os.RemoveAll(tempDir)
	}

	if err := extractMethodologySubtree(resp.Body, tempDir); err != nil {
		cleanup()
		return "", nil, err
	}

	if _, err := os.Stat(filepath.Join(tempDir, "project_root", "manifest.json")); err != nil {
		cleanup()
		if os.IsNotExist(err) {
			return "", nil, fmt.Errorf("methodology payload missing project_root/manifest.json")
		}
		return "", nil, fmt.Errorf("verify extracted payload: %w", err)
	}

	return tempDir, cleanup, nil
}

func extractMethodologySubtree(tarGz io.Reader, destination string) error {
	gzipReader, err := gzip.NewReader(tarGz)
	if err != nil {
		return fmt.Errorf("open tarball stream: %w", err)
	}
	defer gzipReader.Close()

	tarReader := tar.NewReader(gzipReader)
	var extracted bool

	for {
		header, err := tarReader.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("read tarball entry: %w", err)
		}

		name := filepath.ToSlash(strings.TrimPrefix(header.Name, "./"))
		parts := strings.Split(name, "/")
		if len(parts) < 3 || parts[1] != "methodology" {
			continue
		}

		rel := strings.Join(parts[2:], "/")
		rel = strings.TrimSpace(rel)
		if rel == "" {
			continue
		}
		if strings.Contains(rel, "..") {
			return fmt.Errorf("invalid methodology entry path %q", rel)
		}

		targetPath := filepath.Join(destination, filepath.FromSlash(rel))
		targetPath = filepath.Clean(targetPath)
		if !strings.HasPrefix(targetPath, filepath.Clean(destination)+string(os.PathSeparator)) && targetPath != filepath.Clean(destination) {
			return fmt.Errorf("invalid methodology target path %q", targetPath)
		}

		switch header.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(targetPath, 0o755); err != nil {
				return fmt.Errorf("create extracted directory %q: %w", targetPath, err)
			}
			extracted = true
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(targetPath), 0o755); err != nil {
				return fmt.Errorf("create extracted parent %q: %w", filepath.Dir(targetPath), err)
			}

			mode := os.FileMode(0o644)
			if header.Mode > 0 {
				mode = os.FileMode(header.Mode & 0o777)
			}

			out, err := os.OpenFile(targetPath, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, mode)
			if err != nil {
				return fmt.Errorf("open extracted file %q: %w", targetPath, err)
			}

			if _, err := io.Copy(out, tarReader); err != nil {
				out.Close()
				return fmt.Errorf("write extracted file %q: %w", targetPath, err)
			}
			if err := out.Close(); err != nil {
				return fmt.Errorf("close extracted file %q: %w", targetPath, err)
			}
			extracted = true
		}
	}

	if !extracted {
		return fmt.Errorf("tarball did not contain methodology payload")
	}

	return nil
}

func SyncToProject(sourceDir string, projectRoot string) (string, error) {
	destination := filepath.Join(projectRoot, ".methodology")
	if err := copyDir(sourceDir, destination); err != nil {
		return "", err
	}

	hashes, err := dirFileHashes(destination)
	if err != nil {
		return "", err
	}

	if err := writeSyncState(destination, hashes); err != nil {
		return "", err
	}

	return destination, nil
}

func copyDir(src string, dst string) error {
	entries, err := os.ReadDir(src)
	if err != nil {
		return fmt.Errorf("read source directory %q: %w", src, err)
	}

	if err := os.MkdirAll(dst, 0o755); err != nil {
		return fmt.Errorf("create destination directory %q: %w", dst, err)
	}

	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, entry.Name())

		if entry.IsDir() {
			if err := copyDir(srcPath, dstPath); err != nil {
				return err
			}
			continue
		}

		if err := copyFile(srcPath, dstPath); err != nil {
			return err
		}
	}

	return nil
}

func copyFile(src string, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return fmt.Errorf("open source file %q: %w", src, err)
	}
	defer in.Close()

	info, err := in.Stat()
	if err != nil {
		return fmt.Errorf("stat source file %q: %w", src, err)
	}

	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return fmt.Errorf("create destination parent %q: %w", filepath.Dir(dst), err)
	}

	out, err := os.OpenFile(dst, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, info.Mode().Perm())
	if err != nil {
		return fmt.Errorf("open destination file %q: %w", dst, err)
	}
	defer out.Close()

	if _, err := io.Copy(out, in); err != nil {
		return fmt.Errorf("copy %q to %q: %w", src, dst, err)
	}

	return nil
}
