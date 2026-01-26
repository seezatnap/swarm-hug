# Lima Connect Script

## Overview

Create a shell script (`lima_connect.sh`) that provides an interactive way to connect to Docker containers running inside Lima VMs.

## Problem

Connecting to a Lima Docker container requires multiple steps:
1. Know the Lima VM name
2. Derive or look up the Docker context name
3. List containers in that context
4. Run the exec command with correct context and container name

This is tedious for daily use.

## Solution

A single script that automates discovery and connection.

## Workflow

1. Use `limactl list` to discover running Lima VMs
2. For each VM, derive the Docker context name (`lima-<vm_name>`)
3. Use `docker --context <context> ps` to list running containers
4. Present an interactive menu to select a container
5. Execute `docker --context <context> exec -it <container_name> bash -l`

## Key Commands Reference

```bash
# List Lima VMs (name only)
limactl list --format '{{.Name}}'

# List Lima VMs with status
limactl list --format '{{.Name}} {{.Status}}'

# Get Docker socket path for a specific VM
limactl list <vm_name> --format 'unix://{{.Dir}}/sock/docker.sock'

# Docker context naming convention (from init_lima.sh line 139)
CTX="lima-${VM_NAME}"

# Verify context exists
docker context inspect "lima-<vm_name>" >/dev/null 2>&1

# Create context if missing
docker context create "lima-<vm_name>" --docker "host=unix://..."

# List running containers using context
docker --context lima-<vm_name> ps --format '{{.Names}}'

# List all containers (including stopped)
docker --context lima-<vm_name> ps -a --format '{{.Names}}\t{{.Status}}'

# Connect to container
docker --context lima-<vm_name> exec -it <container_name> bash -l
```

## Requirements

### Must Have
- [ ] Executable script at repo root: `lima_connect.sh`
- [ ] List all running Lima VMs
- [ ] For selected VM, list all running containers
- [ ] Interactive selection (use bash `select` builtin)
- [ ] Connect with `docker exec -it ... bash -l`
- [ ] Error handling for: no VMs running, no containers running, missing docker context

### Should Have
- [ ] Auto-connect if only one VM and one container exist (skip menus)
- [ ] Use `fzf` for selection if available, fall back to `select`
- [ ] Show container status alongside name in menu

### Nice to Have
- [ ] `--vm <name>` flag to skip VM selection
- [ ] `--container <name>` flag to skip container selection
- [ ] `--shell <cmd>` flag to override default `bash -l`

## Shell Conventions

Follow the same patterns as `init_lima.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

die() { printf "Error: %s\n" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
```

## Example Usage

```bash
# Interactive mode - prompts for VM and container
./lima_connect.sh

# Direct connect if only one option at each level
./lima_connect.sh  # auto-selects if unambiguous

# Future: with flags
./lima_connect.sh --vm swarmhug --container swarmbox-agent
```

## Output Location

`~/swarm-hug/lima_connect.sh`

## Testing

1. With one Lima VM running one container - should auto-connect
2. With multiple VMs - should prompt for selection
3. With no VMs running - should show helpful error
4. With VM but no containers - should show helpful error
