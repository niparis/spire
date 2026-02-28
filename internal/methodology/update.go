package methodology

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
)

const syncStateFilename = ".spire-sync-state.json"

type syncState struct {
	Hashes map[string]string `json:"hashes"`
}

func DetectDirty(localDir string) ([]string, error) {
	localHashes, err := dirFileHashes(localDir)
	if err != nil {
		return nil, err
	}

	state, err := readSyncState(localDir)
	if err != nil {
		return nil, err
	}
	if state == nil {
		return nil, nil
	}

	var dirty []string
	for path, sourceHash := range state.Hashes {
		localHash, ok := localHashes[path]
		if !ok || localHash != sourceHash {
			dirty = append(dirty, path)
		}
	}

	for path := range localHashes {
		if _, ok := state.Hashes[path]; !ok {
			dirty = append(dirty, path)
		}
	}

	sort.Strings(dirty)
	return dedupeSorted(dirty), nil
}

func SyncAndReportChanges(localDir string, sourceDir string) ([]string, error) {
	beforeHashes, err := dirFileHashes(localDir)
	if err != nil {
		return nil, err
	}

	if err := copyDir(sourceDir, localDir); err != nil {
		return nil, err
	}

	afterHashes, err := dirFileHashes(localDir)
	if err != nil {
		return nil, err
	}

	var changed []string
	for path, afterHash := range afterHashes {
		beforeHash, ok := beforeHashes[path]
		if !ok || beforeHash != afterHash {
			changed = append(changed, path)
		}
	}

	for path := range beforeHashes {
		if _, ok := afterHashes[path]; !ok {
			changed = append(changed, path)
		}
	}

	if err := writeSyncState(localDir, afterHashes); err != nil {
		return nil, err
	}

	sort.Strings(changed)
	return dedupeSorted(changed), nil
}

func dirFileHashes(root string) (map[string]string, error) {
	hashes := map[string]string{}

	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}

		if d.IsDir() {
			return nil
		}

		content, err := os.ReadFile(path)
		if err != nil {
			return err
		}

		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		if rel == syncStateFilename || rel == sourceMetadataFilename {
			return nil
		}

		sum := sha256.Sum256(content)
		hashes[rel] = hex.EncodeToString(sum[:])
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("scan directory %q: %w", root, err)
	}

	return hashes, nil
}

func dedupeSorted(items []string) []string {
	if len(items) == 0 {
		return items
	}

	result := []string{items[0]}
	for i := 1; i < len(items); i++ {
		if items[i] != items[i-1] {
			result = append(result, items[i])
		}
	}

	return result
}

func syncStatePath(localDir string) string {
	return filepath.Join(localDir, syncStateFilename)
}

func readSyncState(localDir string) (*syncState, error) {
	data, err := os.ReadFile(syncStatePath(localDir))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("read sync state: %w", err)
	}

	var state syncState
	if err := json.Unmarshal(data, &state); err != nil {
		return nil, fmt.Errorf("parse sync state: %w", err)
	}

	if state.Hashes == nil {
		state.Hashes = map[string]string{}
	}

	return &state, nil
}

func writeSyncState(localDir string, hashes map[string]string) error {
	state := syncState{Hashes: hashes}
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return fmt.Errorf("serialize sync state: %w", err)
	}

	if err := os.WriteFile(syncStatePath(localDir), data, 0o644); err != nil {
		return fmt.Errorf("write sync state: %w", err)
	}

	return nil
}
