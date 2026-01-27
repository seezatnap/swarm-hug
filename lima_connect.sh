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

# TODO: Implement VM discovery, container listing, and interactive selection
