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

# --- VM selection ---
# Auto-select if only one VM, otherwise prompt with select menu
select_vm() {
  local vms=("$@")
  local count=${#vms[@]}

  if [[ $count -eq 1 ]]; then
    printf '%s\n' "${vms[0]}"
    return
  fi

  printf "Select a Lima VM:\n" >&2
  PS3="VM number: "
  select vm in "${vms[@]}"; do
    if [[ -n "$vm" ]]; then
      printf '%s\n' "$vm"
      return
    fi
    printf "Invalid selection. Please try again.\n" >&2
  done
}

SELECTED_VM="$(select_vm "${RUNNING_VMS[@]}")"
[[ -n "$SELECTED_VM" ]] || die "No VM selected"

# Ensure docker context exists for the selected VM
DOCKER_CTX="$(ensure_docker_context "$SELECTED_VM")"
[[ -n "$DOCKER_CTX" ]] || die "Failed to get docker context for VM: $SELECTED_VM"

# --- Container discovery ---
# List running container names for a given docker context
list_running_containers() {
  local ctx="$1"
  docker --context "$ctx" ps --format '{{.Names}}' 2>/dev/null
}

# Get container status info (for menu display)
# Returns lines of "container_name<TAB>status"
get_container_status() {
  local ctx="$1"
  docker --context "$ctx" ps -a --format '{{.Names}}\t{{.Status}}' 2>/dev/null
}

# Discover running containers and their status for menu display
# Populates two parallel arrays: CONTAINER_NAMES and CONTAINER_STATUSES
discover_containers() {
  local ctx="$1"
  local -a names=()
  local -a statuses=()
  local line name status

  # Get all containers with status
  while IFS=$'\t' read -r name status; do
    [[ -z "$name" ]] && continue
    names+=("$name")
    statuses+=("$status")
  done < <(get_container_status "$ctx")

  # Export via global arrays (bash limitation for returning arrays)
  CONTAINER_NAMES=("${names[@]+"${names[@]}"}")
  CONTAINER_STATUSES=("${statuses[@]+"${statuses[@]}"}")
}

# Get only running container names (for filtering)
get_running_container_names() {
  local ctx="$1"
  list_running_containers "$ctx"
}

CONTAINER_NAMES=()
CONTAINER_STATUSES=()
discover_containers "$DOCKER_CTX"

# Check if there are any containers at all
[[ ${#CONTAINER_NAMES[@]} -gt 0 ]] || die "No containers found in VM: $SELECTED_VM (context: $DOCKER_CTX)"

# Filter to only running containers while preserving status info for display
RUNNING_CONTAINER_NAMES=()
RUNNING_CONTAINER_DISPLAY=()
for i in "${!CONTAINER_NAMES[@]}"; do
  name="${CONTAINER_NAMES[$i]}"
  status="${CONTAINER_STATUSES[$i]}"
  # Check if status starts with "Up" (running container)
  if [[ "$status" == Up* ]]; then
    RUNNING_CONTAINER_NAMES+=("$name")
    RUNNING_CONTAINER_DISPLAY+=("$name ($status)")
  fi
done

[[ ${#RUNNING_CONTAINER_NAMES[@]} -gt 0 ]] || die "No running containers found in VM: $SELECTED_VM (context: $DOCKER_CTX). Start a container first."

# TODO: Implement container selection menu and exec
