#!/usr/bin/env bash
set -euo pipefail

# lima_connect.sh — Connect to Docker containers running inside Lima VMs
#
# Usage:
#   ./lima_connect.sh
#
# See specs.md for full workflow and requirements.

die() { printf "Error: %s\n" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# --- preflight checks ---
have limactl || die "Missing limactl (Lima). Install: brew install lima"
have docker  || die "Missing docker CLI. Install Docker Desktop (or: brew install docker)"

# --- VM discovery ---
# Discover running Lima VMs via limactl list
# Output format: "name status" per line, filter for Running status
discover_running_vms() {
  local vms=()
  local line name status
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    name="${line%% *}"
    status="${line#* }"
    if [[ "$status" == "Running" ]]; then
      vms+=("$name")
    fi
  done < <(limactl list --format '{{.Name}} {{.Status}}' 2>/dev/null)
  printf '%s\n' "${vms[@]}"
}

RUNNING_VMS=()
while IFS= read -r vm; do
  [[ -n "$vm" ]] && RUNNING_VMS+=("$vm")
done < <(discover_running_vms)

[[ ${#RUNNING_VMS[@]} -gt 0 ]] || die "No running Lima VMs found. Start a VM with: limactl start <name>"

# --- Docker socket and context ---
# Resolve the docker socket path for a given VM
get_docker_socket() {
  local vm="$1"
  limactl list "$vm" --format 'unix://{{.Dir}}/sock/docker.sock' 2>/dev/null
}

# Ensure docker context exists for a VM, creating it if necessary
# Returns the context name (lima-<vm>)
ensure_docker_context() {
  local vm="$1"
  local ctx="lima-${vm}"
  local sock

  sock="$(get_docker_socket "$vm")" || die "Failed to get docker socket for VM: $vm"
  [[ -n "$sock" ]] || die "Empty docker socket path for VM: $vm"

  if ! docker context inspect "$ctx" >/dev/null 2>&1; then
    docker context create "$ctx" --docker "host=${sock}" >/dev/null \
      || die "Failed to create docker context: $ctx"
  fi

  printf '%s\n' "$ctx"
}

# TODO: Implement container listing and interactive selection
