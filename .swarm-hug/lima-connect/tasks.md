# Tasks

## Script Foundation
- [x] (#1) Create `lima_connect.sh` scaffold at repo root with shebang, `set -euo pipefail`, `die()`/`have()`, and make it executable (A)

## Lima & Docker Discovery
- [x] (#2) Implement running VM discovery via `limactl list --format '{{.Name}} {{.Status}}'` and error when none running (blocked by #1) (A)
- [x] (#3) Resolve docker socket for chosen VM via `limactl list <vm> --format 'unix://{{.Dir}}/sock/docker.sock'` and ensure `lima-<vm>` context exists (inspect/create) (blocked by #2) (A)

## Containers
- [x] (#4) List running containers via `docker --context <ctx> ps` and collect status info (via `ps -a`) for menu display (blocked by #3) (A)

## Selection UX
- [x] (#5) Add VM selection menu using bash `select`, auto-select when only one VM (blocked by #2) (A)
- [x] (#6) Add container selection menu showing name + status, auto-select when only one running container (blocked by #4, #5) (A)
- [A] (#7) Use `fzf` for VM/container selection when available, with `select` fallback (blocked by #5, #6)

## Flags & Execution
- [A] (#8) Parse `--vm`, `--container`, and `--shell` flags to skip selections and override shell (blocked by #5, #6)
- [ ] (#9) Execute `docker --context <ctx> exec -it <container> <shell>` defaulting to `bash -l` (blocked by #3, #6, #8)

## Error Handling
- [ ] (#10) Add explicit errors for missing docker context, no running containers, or invalid provided VM/container names (blocked by #3, #4, #8)

## Testing
- [ ] (#11) Run and document manual test scenarios from PRD (single VM+container auto-connect, multiple VMs prompt, no VMs error, VM with no containers error) (blocked by #9, #10)
