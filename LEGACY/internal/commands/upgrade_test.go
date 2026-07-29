package commands

import (
	"bytes"
	"fmt"
	"strings"
	"testing"
)

func TestRunUpgradeRejectsUnexpectedArgs(t *testing.T) {
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade([]string{"--check"}, "0.2.0", &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "usage: spire upgrade") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunUpgradeNoopWhenLatestIsNotNewer(t *testing.T) {
	restore := overrideUpgradeDeps(t, latestRelease{TagName: "v0.2.0", Assets: []releaseAsset{{Name: "spire_darwin_arm64", URL: "https://example.invalid/spire"}}}, nil)
	defer restore()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade(nil, "0.2.0", &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "up to date") {
		t.Fatalf("stdout: %q", stdout.String())
	}
}

func TestRunUpgradeWhenNewerReleaseAvailable(t *testing.T) {
	var replacedWith string
	restore := overrideUpgradeDeps(t, latestRelease{TagName: "v0.3.0", Assets: []releaseAsset{{Name: "spire_darwin_arm64", URL: "https://example.invalid/spire-new"}}}, func(url string) error {
		replacedWith = url
		return nil
	})
	defer restore()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade(nil, "0.2.0", &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if replacedWith != "https://example.invalid/spire-new" {
		t.Fatalf("replace url: got %q", replacedWith)
	}
	if !strings.Contains(stdout.String(), "upgraded spire from 0.2.0 to 0.3.0") {
		t.Fatalf("stdout: %q", stdout.String())
	}
}

func TestRunUpgradeFailsWhenAssetMissing(t *testing.T) {
	restore := overrideUpgradeDeps(t, latestRelease{TagName: "v0.3.0", Assets: []releaseAsset{{Name: "spire_windows_amd64.exe", URL: "https://example.invalid/spire.exe"}}}, nil)
	defer restore()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade(nil, "0.2.0", &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "asset \"spire_darwin_arm64\" was not published") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func TestRunUpgradeFromDevBuild(t *testing.T) {
	var called bool
	restore := overrideUpgradeDeps(t, latestRelease{TagName: "v0.4.0", Assets: []releaseAsset{{Name: "spire_darwin_arm64", URL: "https://example.invalid/spire-new"}}}, func(url string) error {
		called = true
		return nil
	})
	defer restore()

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade(nil, "dev", &stdout, &stderr)

	if exitCode != 0 {
		t.Fatalf("exit code: got %d, stderr=%q", exitCode, stderr.String())
	}
	if !called {
		t.Fatal("expected replacement to be called")
	}
	if !strings.Contains(stdout.String(), "upgraded spire from dev to 0.4.0") {
		t.Fatalf("stdout: %q", stdout.String())
	}
}

func TestRunUpgradeFetchFailure(t *testing.T) {
	prevFetch := fetchRelease
	fetchRelease = func(repo string) (latestRelease, error) {
		return latestRelease{}, fmt.Errorf("boom")
	}
	t.Cleanup(func() { fetchRelease = prevFetch })

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := RunUpgrade(nil, "0.2.0", &stdout, &stderr)

	if exitCode != 1 {
		t.Fatalf("exit code: got %d, want 1", exitCode)
	}
	if !strings.Contains(stderr.String(), "failed to check latest release") {
		t.Fatalf("stderr: %q", stderr.String())
	}
}

func overrideUpgradeDeps(t *testing.T, release latestRelease, replace func(url string) error) func() {
	t.Helper()

	prevFetch := fetchRelease
	prevReplace := replaceBinary
	prevGOOS := runtimeGOOS
	prevGOARCH := runtimeGOARCH

	fetchRelease = func(repo string) (latestRelease, error) {
		return release, nil
	}
	if replace == nil {
		replaceBinary = func(url string) error {
			t.Fatalf("replaceBinary should not be called")
			return nil
		}
	} else {
		replaceBinary = replace
	}
	runtimeGOOS = "darwin"
	runtimeGOARCH = "arm64"

	return func() {
		fetchRelease = prevFetch
		replaceBinary = prevReplace
		runtimeGOOS = prevGOOS
		runtimeGOARCH = prevGOARCH
	}
}
