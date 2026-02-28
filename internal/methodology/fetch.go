package methodology

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

const envMethodologySource = "SPIRE_METHODOLOGY_SOURCE"

func ResolveSource() (string, error) {
	source := strings.TrimSpace(os.Getenv(envMethodologySource))
	if source == "" {
		return "", fmt.Errorf("%s is not set", envMethodologySource)
	}

	cleaned := filepath.Clean(source)
	info, err := os.Stat(cleaned)
	if err != nil {
		return "", fmt.Errorf("invalid methodology source %q: %w", cleaned, err)
	}
	if !info.IsDir() {
		return "", fmt.Errorf("methodology source %q is not a directory", cleaned)
	}

	return cleaned, nil
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
