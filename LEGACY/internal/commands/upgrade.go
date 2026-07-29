package commands

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
)

const defaultUpgradeRepo = "niparis/spire"

var (
	httpClient      = &http.Client{Timeout: 30 * time.Second}
	runtimeGOOS     = runtime.GOOS
	runtimeGOARCH   = runtime.GOARCH
	fetchRelease    = fetchLatestRelease
	replaceBinary   = replaceCurrentBinary
	upgradeRepoName = defaultUpgradeRepo
)

type releaseAsset struct {
	Name string `json:"name"`
	URL  string `json:"browser_download_url"`
}

type latestRelease struct {
	TagName string         `json:"tag_name"`
	Assets  []releaseAsset `json:"assets"`
}

type semver struct {
	major int
	minor int
	patch int
}

func RunUpgrade(args []string, currentVersion string, stdout io.Writer, stderr io.Writer) int {
	if len(args) > 0 {
		fmt.Fprintln(stderr, "usage: spire upgrade")
		return 1
	}

	release, err := fetchRelease(upgradeRepoName)
	if err != nil {
		fmt.Fprintf(stderr, "failed to check latest release: %v\n", err)
		return 1
	}

	latest, err := parseSemver(release.TagName)
	if err != nil {
		fmt.Fprintf(stderr, "failed to parse latest version %q: %v\n", release.TagName, err)
		return 1
	}

	current, currentErr := parseSemver(currentVersion)
	if currentErr == nil && compareSemver(latest, current) <= 0 {
		fmt.Fprintf(stdout, "spire is up to date (%s)\n", normalizeVersion(currentVersion))
		return 0
	}

	assetName, err := assetNameForPlatform(runtimeGOOS, runtimeGOARCH)
	if err != nil {
		fmt.Fprintf(stderr, "cannot upgrade on this platform: %v\n", err)
		return 1
	}

	assetURL, err := findAssetURL(release.Assets, assetName)
	if err != nil {
		fmt.Fprintf(stderr, "failed to find downloadable asset: %v\n", err)
		return 1
	}

	if err := replaceBinary(assetURL); err != nil {
		fmt.Fprintf(stderr, "failed to replace current executable: %v\n", err)
		return 1
	}

	fromVersion := normalizeVersion(currentVersion)
	if currentErr != nil {
		fromVersion = currentVersion
	}

	fmt.Fprintf(stdout, "upgraded spire from %s to %s\n", fromVersion, normalizeVersion(release.TagName))
	return 0
}

func fetchLatestRelease(repo string) (latestRelease, error) {
	url := fmt.Sprintf("https://api.github.com/repos/%s/releases/latest", repo)
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return latestRelease{}, fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	req.Header.Set("User-Agent", "spire-upgrade")

	resp, err := httpClient.Do(req)
	if err != nil {
		return latestRelease{}, fmt.Errorf("request latest release: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return latestRelease{}, fmt.Errorf("unexpected status %s", resp.Status)
	}

	var release latestRelease
	if err := json.NewDecoder(resp.Body).Decode(&release); err != nil {
		return latestRelease{}, fmt.Errorf("decode latest release response: %w", err)
	}

	if strings.TrimSpace(release.TagName) == "" {
		return latestRelease{}, fmt.Errorf("latest release did not include a tag")
	}

	return release, nil
}

func replaceCurrentBinary(downloadURL string) error {
	execPath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("resolve current executable: %w", err)
	}

	info, err := os.Stat(execPath)
	if err != nil {
		return fmt.Errorf("stat current executable: %w", err)
	}

	dir := filepath.Dir(execPath)
	tmp, err := os.CreateTemp(dir, "spire-upgrade-*")
	if err != nil {
		return fmt.Errorf("create temp file in executable directory: %w", err)
	}
	tmpPath := tmp.Name()

	cleanup := func() {
		tmp.Close()
		_ = os.Remove(tmpPath)
	}

	req, err := http.NewRequest(http.MethodGet, downloadURL, nil)
	if err != nil {
		cleanup()
		return fmt.Errorf("create download request: %w", err)
	}
	req.Header.Set("User-Agent", "spire-upgrade")

	resp, err := httpClient.Do(req)
	if err != nil {
		cleanup()
		return fmt.Errorf("download replacement binary: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		cleanup()
		return fmt.Errorf("download replacement binary: unexpected status %s", resp.Status)
	}

	if _, err := io.Copy(tmp, resp.Body); err != nil {
		cleanup()
		return fmt.Errorf("write replacement binary: %w", err)
	}

	if err := tmp.Chmod(info.Mode().Perm()); err != nil {
		cleanup()
		return fmt.Errorf("chmod replacement binary: %w", err)
	}

	if err := tmp.Close(); err != nil {
		cleanup()
		return fmt.Errorf("close replacement binary: %w", err)
	}

	if err := os.Rename(tmpPath, execPath); err != nil {
		cleanup()
		return fmt.Errorf("swap binary at %q (check file permissions): %w", execPath, err)
	}

	return nil
}

func assetNameForPlatform(goos string, goarch string) (string, error) {
	if goos == "windows" {
		return fmt.Sprintf("spire_%s_%s.exe", goos, goarch), nil
	}

	supportedOS := goos == "darwin" || goos == "linux"
	supportedArch := goarch == "arm64" || goarch == "amd64"
	if !supportedOS || !supportedArch {
		return "", fmt.Errorf("unsupported platform %s/%s", goos, goarch)
	}

	return fmt.Sprintf("spire_%s_%s", goos, goarch), nil
}

func findAssetURL(assets []releaseAsset, assetName string) (string, error) {
	for _, asset := range assets {
		if asset.Name == assetName {
			if strings.TrimSpace(asset.URL) == "" {
				return "", fmt.Errorf("asset %q has empty download URL", assetName)
			}
			return asset.URL, nil
		}
	}

	return "", fmt.Errorf("asset %q was not published in latest release", assetName)
}

func parseSemver(raw string) (semver, error) {
	clean := normalizeVersion(raw)
	parts := strings.Split(clean, ".")
	if len(parts) != 3 {
		return semver{}, fmt.Errorf("invalid semantic version %q", raw)
	}

	major, err := strconv.Atoi(parts[0])
	if err != nil {
		return semver{}, fmt.Errorf("invalid major version: %w", err)
	}
	minor, err := strconv.Atoi(parts[1])
	if err != nil {
		return semver{}, fmt.Errorf("invalid minor version: %w", err)
	}
	patch, err := strconv.Atoi(parts[2])
	if err != nil {
		return semver{}, fmt.Errorf("invalid patch version: %w", err)
	}

	return semver{major: major, minor: minor, patch: patch}, nil
}

func normalizeVersion(raw string) string {
	clean := strings.TrimSpace(raw)
	clean = strings.TrimPrefix(clean, "v")
	return clean
}

func compareSemver(a semver, b semver) int {
	if a.major != b.major {
		if a.major > b.major {
			return 1
		}
		return -1
	}
	if a.minor != b.minor {
		if a.minor > b.minor {
			return 1
		}
		return -1
	}
	if a.patch != b.patch {
		if a.patch > b.patch {
			return 1
		}
		return -1
	}
	return 0
}
