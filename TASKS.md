# Tasks

## Flags & Execution
- [x] (#8) Parse `--vm`, `--container`, and `--shell` flags to skip selections and override shell
- [x] (#9) Execute `docker --context <ctx> exec -it <container> <shell>` defaulting to `bash -l`

## Error Handling
- [x] (#10) Add explicit errors for missing docker context, no running containers, or invalid provided VM/container names
