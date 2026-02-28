package commands

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
)

var featureFilePattern = regexp.MustCompile(`^feature-(\d+)-(.+)\.md$`)

func RunNew(args []string, projectRoot string, stdin io.Reader, stdout io.Writer, stderr io.Writer) int {
	nextNum, err := nextFeatureNumber(projectRoot)
	if err != nil {
		fmt.Fprintf(stderr, "failed to determine next feature number: %v\n", err)
		return 1
	}

	fmt.Fprint(stdout, "Feature name (kebab-case): ")
	reader := bufio.NewReader(stdin)
	rawName, err := reader.ReadString('\n')
	if err != nil && err != io.EOF {
		fmt.Fprintf(stderr, "failed to read feature name: %v\n", err)
		return 1
	}

	name := normalizeFeatureName(rawName)
	if name == "" {
		fmt.Fprintln(stderr, "Name required.")
		return 1
	}

	if duplicate, err := featureNameExists(projectRoot, name); err != nil {
		fmt.Fprintf(stderr, "failed to validate feature name uniqueness: %v\n", err)
		return 1
	} else if duplicate {
		fmt.Fprintf(stderr, "Spec already exists for feature name: %s\n", name)
		return 1
	}

	number := fmt.Sprintf("%03d", nextNum)
	slug := number + "-" + name

	specPath := filepath.Join(projectRoot, "specs", "feature-"+slug+".md")
	if exists, err := pathExists(specPath); err != nil {
		fmt.Fprintf(stderr, "failed to inspect spec destination: %v\n", err)
		return 1
	} else if exists {
		fmt.Fprintf(stderr, "Spec already exists: %s\n", specPath)
		return 1
	}

	specTemplatePath := resolveSpecTemplatePath(projectRoot)
	sessionTemplatePath := filepath.Join(projectRoot, ".methodology", "templates", "session-template.md")

	date := time.Now().Format("2006-01-02")
	specContent, err := renderTemplate(specTemplatePath, name, number, date)
	if err != nil {
		fmt.Fprintf(stderr, "failed to render spec template: %v\n", err)
		return 1
	}

	sessionContent, err := renderTemplate(sessionTemplatePath, name, number, date)
	if err != nil {
		fmt.Fprintf(stderr, "failed to render session template: %v\n", err)
		return 1
	}

	if err := os.MkdirAll(filepath.Join(projectRoot, "specs"), 0o755); err != nil {
		fmt.Fprintf(stderr, "failed to create specs directory: %v\n", err)
		return 1
	}
	if err := os.WriteFile(specPath, []byte(specContent), 0o644); err != nil {
		fmt.Fprintf(stderr, "failed to create spec: %v\n", err)
		return 1
	}

	changesDir := filepath.Join(projectRoot, "changes", slug)
	if err := os.MkdirAll(changesDir, 0o755); err != nil {
		fmt.Fprintf(stderr, "failed to create changes directory: %v\n", err)
		return 1
	}

	sessionPath := filepath.Join(changesDir, "SESSION.md")
	if err := os.WriteFile(sessionPath, []byte(sessionContent), 0o644); err != nil {
		fmt.Fprintf(stderr, "failed to create session log: %v\n", err)
		return 1
	}

	fmt.Fprintf(stdout, "Created: %s\n", specPath)
	fmt.Fprintf(stdout, "Created: %s\n", sessionPath)
	fmt.Fprintln(stdout)
	fmt.Fprintln(stdout, "Next: fill in the spec, then run the Plan Agent.")

	return 0
}

func nextFeatureNumber(projectRoot string) (int, error) {
	specsDir := filepath.Join(projectRoot, "specs")
	entries, err := os.ReadDir(specsDir)
	if err != nil {
		if os.IsNotExist(err) {
			return 1, nil
		}
		return 0, err
	}

	max := 0
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		name := entry.Name()
		if strings.HasSuffix(name, "-AUDIT.md") {
			continue
		}

		matches := featureFilePattern.FindStringSubmatch(name)
		if len(matches) != 3 {
			continue
		}

		n, err := strconv.Atoi(matches[1])
		if err != nil {
			continue
		}
		if n > max {
			max = n
		}
	}

	return max + 1, nil
}

func featureNameExists(projectRoot string, featureName string) (bool, error) {
	specsDir := filepath.Join(projectRoot, "specs")
	entries, err := os.ReadDir(specsDir)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, err
	}

	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		name := entry.Name()
		if strings.HasSuffix(name, "-AUDIT.md") {
			continue
		}

		matches := featureFilePattern.FindStringSubmatch(name)
		if len(matches) != 3 {
			continue
		}

		if matches[2] == featureName {
			return true, nil
		}
	}

	return false, nil
}

func normalizeFeatureName(raw string) string {
	v := strings.ToLower(strings.TrimSpace(raw))
	v = strings.ReplaceAll(v, "_", "-")
	v = strings.Join(strings.Fields(v), "-")

	var b strings.Builder
	prevDash := false
	for _, r := range v {
		isAlphaNum := (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9')
		if isAlphaNum {
			b.WriteRune(r)
			prevDash = false
			continue
		}

		if !prevDash {
			b.WriteRune('-')
			prevDash = true
		}
	}

	result := b.String()
	result = strings.Trim(result, "-")
	if result == "" {
		return ""
	}

	parts := strings.Split(result, "-")
	filtered := make([]string, 0, len(parts))
	for _, part := range parts {
		if part != "" {
			filtered = append(filtered, part)
		}
	}

	return strings.Join(filtered, "-")
}

func resolveSpecTemplatePath(projectRoot string) string {
	preferred := filepath.Join(projectRoot, "specs", "_template.md")
	if exists, _ := pathExists(preferred); exists {
		return preferred
	}
	return filepath.Join(projectRoot, ".methodology", "templates", "spec-template.md")
}

func renderTemplate(path string, featureName string, number string, date string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}

	content := string(data)
	replacements := map[string]string{
		"[Feature Name]": featureName,
		"[NUMBER]":       number,
		"YYYY-MM-DD":     date,
	}

	keys := make([]string, 0, len(replacements))
	for key := range replacements {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	for _, key := range keys {
		content = strings.ReplaceAll(content, key, replacements[key])
	}

	return content, nil
}

func pathExists(path string) (bool, error) {
	_, err := os.Stat(path)
	if err == nil {
		return true, nil
	}
	if os.IsNotExist(err) {
		return false, nil
	}
	return false, err
}
