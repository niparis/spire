package cli

import (
	"fmt"
	"io"
	"os"

	"opencode-spire/internal/commands"
)

var Version = "dev"

func Execute(args []string, stdout io.Writer, stderr io.Writer) int {
	if len(args) == 0 {
		printHelp(stdout)
		return 0
	}

	switch args[0] {
	case "--help", "-h", "help":
		printHelp(stdout)
		return 0
	case "--version", "-v":
		fmt.Fprintf(stdout, "spire %s\n", Version)
		return 0
	case "init":
		cwd, err := os.Getwd()
		if err != nil {
			fmt.Fprintf(stderr, "failed to determine working directory: %v\n", err)
			return 1
		}
		return commands.RunInit(args[1:], cwd, stdout, stderr)
	case "update":
		cwd, err := os.Getwd()
		if err != nil {
			fmt.Fprintf(stderr, "failed to determine working directory: %v\n", err)
			return 1
		}
		return commands.RunUpdate(args[1:], cwd, os.Stdin, isInteractiveStdin(os.Stdin), stdout, stderr)
	case "new":
		cwd, err := os.Getwd()
		if err != nil {
			fmt.Fprintf(stderr, "failed to determine working directory: %v\n", err)
			return 1
		}
		return commands.RunNew(args[1:], cwd, os.Stdin, stdout, stderr)
	case "status":
		cwd, err := os.Getwd()
		if err != nil {
			fmt.Fprintf(stderr, "failed to determine working directory: %v\n", err)
			return 1
		}
		return commands.RunStatus(args[1:], cwd, stdout, stderr)
	default:
		fmt.Fprintf(stderr, "unknown command: %s\n\n", args[0])
		printHelp(stderr)
		return 1
	}
}

func printHelp(w io.Writer) {
	fmt.Fprintln(w, "spire - SDD methodology CLI")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Usage:")
	fmt.Fprintln(w, "  spire <command>")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Commands:")
	fmt.Fprintln(w, "  init      Initialize project methodology")
	fmt.Fprintln(w, "  update    Update local methodology")
	fmt.Fprintln(w, "  new       Create a new feature spec")
	fmt.Fprintln(w, "  status    Show feature status table")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Flags:")
	fmt.Fprintln(w, "  -h, --help       Show help")
	fmt.Fprintln(w, "  -v, --version    Show version")
}

func isInteractiveStdin(file *os.File) bool {
	if file == nil {
		return false
	}
	stat, err := file.Stat()
	if err != nil {
		return false
	}
	return (stat.Mode() & os.ModeCharDevice) != 0
}
