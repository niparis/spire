# systemd transient-runner contract

**Status:** blocked; current host is macOS and has no `systemd-run`
**Last checked:** 2026-07-29

The current workstation cannot validate the production runner contract. The target
VM must demonstrate the following before Sprint 05 starts.

## Required contract

- Unit name: `spire-run-<lowercase-uuid-with-hyphens>.service`.
- A Run ID may only contain lowercase hexadecimal characters and hyphens; the
  runner rejects any other unit-name input.
- The command is an explicit executable plus argument vector, never shell text.
- `WorkingDirectory` is a canonical Spire-owned path below the configured
  worktree root. Before launch, SQLite identity, the ownership marker, and
  `git worktree list --porcelain` must agree.
- Credentials arrive through systemd credentials or a dedicated runtime file; they
  are not inherited as broad process environment or logged.
- Standard output and error go to a per-run evidence location outside the database
  and worktree cleanup roots.
- Stop sends graceful termination once, then force termination after the recorded
  grace deadline.

## Target-VM proof script

Run the following on the target VM only, using a fake UUID and a harmless sleep
command. Capture redacted `systemctl show` output after each step.

```sh
systemd-run --unit=spire-run-00000000-0000-4000-8000-000000000000 \\
  --property=WorkingDirectory=/var/lib/spire/workspaces \\
  /usr/bin/sleep 120
systemctl show spire-run-00000000-0000-4000-8000-000000000000.service \\
  --property=ActiveState --property=SubState --property=MainPID
# Restart the spike controller, then run the same `systemctl show` command.
systemctl stop spire-run-00000000-0000-4000-8000-000000000000.service
```

The resulting evidence must distinguish a missing unit from an inactive finished
unit, demonstrate rediscovery after controller restart, and prove that repeating
a start request for the same Run does not create another process.
