package scaffold

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// SkillMapping defines a human-invoked skill source within .methodology/skills/
// and its projected destination under .opencode/skills/.
type SkillMapping struct {
	Source      string
	Destination string
}

// HumanInvokedSkills lists the skills that must be discoverable by OpenCode
// under .opencode/skills/ with a spire- prefix. Auto-loaded skills
// (implementation-loop, spec-auditor) are intentionally omitted.
var HumanInvokedSkills = []SkillMapping{
	{Source: "product-definition.md", Destination: ".opencode/skills/spire-product-definition/SKILL.md"},
	{Source: "new-feature/SKILL.md", Destination: ".opencode/skills/spire-new-feature/SKILL.md"},
	{Source: "grill-me/SKILL.md", Destination: ".opencode/skills/spire-grill-me/SKILL.md"},
	{Source: "architecture-definition.md", Destination: ".opencode/skills/spire-architecture-definition/SKILL.md"},
}

// ApplySkillProjections copies each human-invoked skill from the methodology
// directory into the project's .opencode/skills/ tree.
func ApplySkillProjections(methodologyDir string, projectRoot string, out io.Writer) error {
	skillsDir := filepath.Join(methodologyDir, "skills")

	for _, skill := range HumanInvokedSkills {
		sourcePath := filepath.Join(skillsDir, skill.Source)
		if _, err := os.Stat(sourcePath); os.IsNotExist(err) {
			continue
		} else if err != nil {
			return fmt.Errorf("stat skill source %q: %w", skill.Source, err)
		}

		destPath := filepath.Join(projectRoot, skill.Destination)
		if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
			return fmt.Errorf("create skill directory for %q: %w", skill.Destination, err)
		}

		if err := copyFile(sourcePath, destPath); err != nil {
			return fmt.Errorf("copy skill %q: %w", skill.Source, err)
		}

		fmt.Fprintf(out, "created skill: %s\n", skill.Destination)
	}

	return nil
}

// GetExpectedSkillProjections returns the set of skill projection paths.
func GetExpectedSkillProjections() []string {
	projections := make([]string, len(HumanInvokedSkills))
	for i, skill := range HumanInvokedSkills {
		projections[i] = skill.Destination
	}
	return projections
}
