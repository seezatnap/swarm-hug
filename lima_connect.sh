#!/usr/bin/env bash
set -euo pipefail

# lima_connect.sh — Connect to Docker containers running inside Lima VMs
#
# Usage:
#   ./lima_connect.sh [--vm <name>] [--container <name>] [--shell <cmd>]
#
# Options:
#   --vm <name>        Skip VM selection, use specified VM
#   --container <name> Skip container selection, use specified container
#   --shell <cmd>      Override shell (default: bash -l)
#   -h, --help         Show this help
#
# See specs.md for full workflow and requirements.

die() { printf "Error: %s\n" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  cat <<'USAGE'
Usage:
  ./lima_connect.sh [--vm <name>] [--container <name>] [--shell <cmd>]

Options:
  --vm <name>        Skip VM selection, use specified VM
  --container <name> Skip container selection, use specified container
  --shell <cmd>      Override shell (default: bash -l)
  -h, --help         Show this help

Examples:
  ./lima_connect.sh                              # Interactive mode
  ./lima_connect.sh --vm swarmbox                # Skip VM selection
  ./lima_connect.sh --container myapp            # Skip container selection
  ./lima_connect.sh --shell /bin/sh              # Use /bin/sh instead of bash -l
  ./lima_connect.sh --vm swarmbox --container myapp --shell zsh
USAGE
}

# --- argument parsing ---
OPT_VM=""
OPT_CONTAINER=""
OPT_SHELL="bash -l"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --vm) OPT_VM="${2:-}"; shift 2 ;;
    --container) OPT_CONTAINER="${2:-}"; shift 2 ;;
    --shell) OPT_SHELL="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    --) shift; break ;;
    -* ) die "Unknown option: $1" ;;
    * ) die "Unexpected argument: $1" ;;
  esac
done

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

  sock="$(get_docker_socket "$vm")" || {
    printf "Error: Failed to get docker socket for VM '%s'.\n" "$vm" >&2
    printf "The VM may not be fully started or Docker may not be configured.\n" >&2
    printf "Try: limactl shell %s -- docker info\n" "$vm" >&2
    exit 1
  }
  if [[ -z "$sock" ]]; then
    printf "Error: Empty docker socket path for VM '%s'.\n" "$vm" >&2
    printf "This usually means Lima is not configured with Docker support.\n" >&2
    printf "Ensure your Lima VM template includes Docker.\n" >&2
    exit 1
  fi

  if ! docker context inspect "$ctx" >/dev/null 2>&1; then
    if ! docker context create "$ctx" --docker "host=${sock}" >/dev/null 2>&1; then
      printf "Error: Failed to create docker context '%s'.\n" "$ctx" >&2
      printf "Socket path: %s\n" "$sock" >&2
      printf "Check that:\n" >&2
      printf "  1. The Lima VM '%s' is running: limactl list\n" "$vm" >&2
      printf "  2. Docker socket exists: ls -la %s\n" "${sock#unix://}" >&2
      printf "  3. You have permission to access the socket\n" >&2
      exit 1
    fi
  fi

  # Verify the context is usable by running a simple docker command
  if ! docker --context "$ctx" info >/dev/null 2>&1; then
    printf "Error: Docker context '%s' exists but is not usable.\n" "$ctx" >&2
    printf "The Docker daemon in VM '%s' may not be running.\n" "$vm" >&2
    printf "Try: limactl shell %s -- docker info\n" "$vm" >&2
    exit 1
  fi

  printf '%s\n' "$ctx"
}

# --- VM selection ---
# Auto-select if only one VM, otherwise prompt with select menu
# Uses fzf when available for better UX, falls back to bash select
select_vm() {
  local vms=("$@")
  local count=${#vms[@]}

  if [[ $count -eq 1 ]]; then
    printf '%s\n' "${vms[0]}"
    return
  fi

  # Use fzf if available
  if have fzf; then
    printf '%s\n' "${vms[@]}" | fzf --prompt="Select a Lima VM: " --height=~50% --reverse
    return
  fi

  # Fallback to bash select
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

# Use --vm flag if provided, otherwise use selection
if [[ -n "$OPT_VM" ]]; then
  # Validate that the specified VM is running
  vm_found=0
  for vm in "${RUNNING_VMS[@]}"; do
    if [[ "$vm" == "$OPT_VM" ]]; then
      vm_found=1
      break
    fi
  done
  if [[ $vm_found -eq 0 ]]; then
    printf "Error: VM '%s' is not running or does not exist.\n" "$OPT_VM" >&2
    printf "Available running VMs:\n" >&2
    for vm in "${RUNNING_VMS[@]}"; do
      printf "  - %s\n" "$vm" >&2
    done
    exit 1
  fi
  SELECTED_VM="$OPT_VM"
else
  SELECTED_VM="$(select_vm "${RUNNING_VMS[@]}")"
  [[ -n "$SELECTED_VM" ]] || die "No VM selected"
fi

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
if [[ ${#CONTAINER_NAMES[@]} -eq 0 ]]; then
  printf "Error: No containers found in VM '%s'.\n" "$SELECTED_VM" >&2
  printf "Docker context: %s\n" "$DOCKER_CTX" >&2
  printf "To see containers: docker --context %s ps -a\n" "$DOCKER_CTX" >&2
  printf "To start a container: docker --context %s run ...\n" "$DOCKER_CTX" >&2
  exit 1
fi

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

if [[ ${#RUNNING_CONTAINER_NAMES[@]} -eq 0 ]]; then
  printf "Error: No running containers found in VM '%s'.\n" "$SELECTED_VM" >&2
  printf "Docker context: %s\n" "$DOCKER_CTX" >&2
  if [[ ${#CONTAINER_NAMES[@]} -gt 0 ]]; then
    printf "Stopped containers exist. To start one:\n" >&2
    for i in "${!CONTAINER_NAMES[@]}"; do
      printf "  docker --context %s start %s\n" "$DOCKER_CTX" "${CONTAINER_NAMES[$i]}" >&2
    done
  else
    printf "To start a container: docker --context %s run ...\n" "$DOCKER_CTX" >&2
  fi
  exit 1
fi

# --- Container selection ---
# Auto-select if only one container, otherwise prompt with select menu
# Shows container name + status for better visibility
# Uses fzf when available for better UX, falls back to bash select
select_container() {
  local -a names=("${!1}")
  local -a display=("${!2}")
  local count=${#names[@]}

  if [[ $count -eq 1 ]]; then
    printf '%s\n' "${names[0]}"
    return
  fi

  # Use fzf if available
  if have fzf; then
    local selected
    selected="$(printf '%s\n' "${display[@]}" | fzf --prompt="Select a container: " --height=~50% --reverse)"
    [[ -z "$selected" ]] && return
    # Find the index of the selected display string to return the name
    local i
    for i in "${!display[@]}"; do
      if [[ "${display[$i]}" == "$selected" ]]; then
        printf '%s\n' "${names[$i]}"
        return
      fi
    done
    return
  fi

  # Fallback to bash select
  printf "Select a container:\n" >&2
  PS3="Container number: "
  select choice in "${display[@]}"; do
    if [[ -n "$choice" ]]; then
      # Find the index of the selected display string
      local i
      for i in "${!display[@]}"; do
        if [[ "${display[$i]}" == "$choice" ]]; then
          printf '%s\n' "${names[$i]}"
          return
        fi
      done
    fi
    printf "Invalid selection. Please try again.\n" >&2
  done
}

# Use --container flag if provided, otherwise use selection
if [[ -n "$OPT_CONTAINER" ]]; then
  # Validate that the specified container exists and is running
  container_found=0
  for name in "${RUNNING_CONTAINER_NAMES[@]}"; do
    if [[ "$name" == "$OPT_CONTAINER" ]]; then
      container_found=1
      break
    fi
  done
  if [[ $container_found -eq 0 ]]; then
    printf "Error: Container '%s' is not running or does not exist in VM '%s'.\n" "$OPT_CONTAINER" "$SELECTED_VM" >&2
    printf "Available running containers:\n" >&2
    for i in "${!RUNNING_CONTAINER_NAMES[@]}"; do
      printf "  - %s (%s)\n" "${RUNNING_CONTAINER_NAMES[$i]}" "${RUNNING_CONTAINER_DISPLAY[$i]#* }" >&2
    done
    exit 1
  fi
  SELECTED_CONTAINER="$OPT_CONTAINER"
else
  SELECTED_CONTAINER="$(select_container RUNNING_CONTAINER_NAMES[@] RUNNING_CONTAINER_DISPLAY[@])"
  [[ -n "$SELECTED_CONTAINER" ]] || die "No container selected"
fi

# --- Execute docker exec ---
# Connect to the selected container with the specified shell
# shellcheck disable=SC2086
exec docker --context "$DOCKER_CTX" exec -it "$SELECTED_CONTAINER" $OPT_SHELL
