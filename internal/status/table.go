package status

import "fmt"

type Row struct {
	Number  string
	Feature string
	Status  string
}

func RenderTable(rows []Row) string {
	if len(rows) == 0 {
		return ""
	}

	output := ""
	output += fmt.Sprintf("%-5s %-35s %s\n", "#", "Feature", "Status")
	output += fmt.Sprintf("%-5s %-35s %s\n", "---", "-----------------------------------", "------")
	for _, row := range rows {
		output += fmt.Sprintf("%-5s %-35s %s\n", row.Number, row.Feature, row.Status)
	}

	return output
}
