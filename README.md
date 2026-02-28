# opencode-spire

`spire` is a Go CLI for managing the SDD methodology lifecycle in projects.

## Install

Installer script (release flow):

```bash
curl -fsSL https://raw.githubusercontent.com/YOU/opencode-spire/main/scripts/install.sh | bash
```

Build locally:

```bash
go run ./cmd/spire --help
```

Release assets:

- macOS Apple Silicon: `spire_darwin_arm64`
- Windows Intel x64: `spire_windows_amd64.exe`

Windows install is manual for now: download the `.exe` from Releases and add it to your `PATH`.

## Commands

| Command | Description |
|---|---|
| `spire init` | Initialize `.methodology/` and scaffold local files |
| `spire update` | Update local methodology content |
| `spire new` | Create a new feature spec and session log |
| `spire status` | Show feature status table |

## Contributing

Start with:

```bash
go test ./...
```
