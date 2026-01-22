#!/usr/bin/env bash
set -euo pipefail

# init.sh — Lima VM + Docker + agent container for swarm-hug
#
# Usage:
#   ./init.sh [--name VM] [--container NAME] [--ports 3000,5173] [--no-auth] <folder1> <folder2> ...
#
# Example:
#   ./init.sh --ports 3000,5173 ~/code/swarm-hug
#
# Notes:
#  - Uses Lima template://docker and --mount-only so the VM only sees the folders you pass.
#  - Runs a hardened container (cap-drop all, read-only rootfs, tmpfs /tmp).
#  - Mounts the swarm-hug repo at /opt/swarm-hug.
#  - Installs codex + claude CLIs in the container for engine support.
#  - Creates an "agent" user with a free UID (>=1000) to avoid UID collisions.

die() { printf "Error: %s\n" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  cat <<'USAGE'
Usage:
  ./init.sh [--name VM] [--container NAME] [--ports 3000,5173] [--no-auth] <folder1> <folder2> ...

Options:
  --name VM            Lima instance name (default: swarmbox)
  --container NAME     Container name (default: swarmbox-agent)
  --ports CSV          Comma-separated ports you run locally on the host (default: 3000)
                       (Printed guidance only; guest reaches host via host.lima.internal)
  --no-auth            Do not drop into an interactive shell to authenticate CLIs
  -h, --help           Show help

Notes:
  - Expects this script to live at the root of the swarm-hug repo.
  - Folders are mounted RW; changes sync back immediately because they are host mounts.
USAGE
}

VM_NAME="swarmbox"
CONTAINER_NAME="swarmbox-agent"
HOST_PORTS="3000"
DO_AUTH="1"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name) VM_NAME="${2:-}"; shift 2 ;;
    --container) CONTAINER_NAME="${2:-}"; shift 2 ;;
    --ports) HOST_PORTS="${2:-}"; shift 2 ;;
    --no-auth) DO_AUTH="0"; shift ;;
    -h|--help) usage; exit 0 ;;
    --) shift; break ;;
    -* ) die "Unknown option: $1" ;;
    * ) break ;;
  esac
done

[[ $# -ge 1 ]] || { usage; die "Pass at least one folder to mount"; }

# --- preflight (host) ---------------------------------------------------------
have limactl || die "Missing limactl (Lima). Install: brew install lima"
have docker  || die "Missing docker CLI. Install Docker Desktop (or: brew install docker)"
have python3 || die "Missing python3 (used for path normalization)."

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
[[ -f "${SCRIPT_DIR}/Cargo.toml" ]] || die "Expected Cargo.toml next to init.sh"

abs_path() {
  python3 - "$1" <<'PY'
import os,sys
print(os.path.realpath(os.path.expanduser(sys.argv[1])))
PY
}

FOLDERS_ABS=()
for p in "$@"; do
  ap="$(abs_path "$p")"
  mkdir -p "$ap"
  FOLDERS_ABS+=("$ap")
done

# Avoid overlapping mounts (Lima disallows overlaps).
dedupe_non_overlapping() {
  python3 - <<'PY' "${FOLDERS_ABS[@]}"
import os,sys
paths=[os.path.normpath(p) for p in sys.argv[1:]]
seen=set()
uniq=[]
for p in paths:
  if p not in seen:
    uniq.append(p); seen.add(p)
uniq.sort(key=lambda x: (len(x), x))
kept=[]
for p in uniq:
  skip=False
  for k in kept:
    if p == k or p.startswith(k + os.sep):
      skip=True; break
  if not skip:
    kept.append(p)
print("\n".join(kept))
PY
}

DEDUPED=()
while IFS= read -r line; do
  [[ -n "$line" ]] && DEDUPED+=("$line")
done < <(dedupe_non_overlapping)
FOLDERS_ABS=("${DEDUPED[@]}")

SCRIPT_DIR_COVERED="0"
for d in "${FOLDERS_ABS[@]}"; do
  if [[ "$SCRIPT_DIR" == "$d" || "$SCRIPT_DIR" == "$d/"* ]]; then
    SCRIPT_DIR_COVERED="1"
    break
  fi
done

# --- create/start Lima VM with mount-only -------------------------------------
if limactl list --format '{{.Name}}' 2>/dev/null | grep -qx "$VM_NAME"; then
  echo "[+] Lima VM exists: $VM_NAME"
  echo "    (If you need different mounts, delete + recreate: limactl delete -f $VM_NAME)"
  limactl start "$VM_NAME" >/dev/null
else
  echo "[+] Creating Lima VM: $VM_NAME"
  mount_args=()
  for d in "${FOLDERS_ABS[@]}"; do
    mount_args+=( "--mount-only" "${d}:w" )
  done
  if [[ "$SCRIPT_DIR_COVERED" == "0" ]]; then
    mount_args+=( "--mount-only" "${SCRIPT_DIR}:w" )
  fi
  limactl start --name "$VM_NAME" template://docker "${mount_args[@]}"
fi

# --- Docker context for this Lima VM ------------------------------------------
DOCKER_HOST_SOCK="$(limactl list "$VM_NAME" --format 'unix://{{.Dir}}/sock/docker.sock')"
CTX="lima-${VM_NAME}"

if ! docker context inspect "$CTX" >/dev/null 2>&1; then
  echo "[+] Creating docker context: $CTX"
  docker context create "$CTX" --docker "host=${DOCKER_HOST_SOCK}" >/dev/null
fi

# --- Build agent image ---------------------------------------------------------
IMAGE="swarm-hug-image:${VM_NAME}"
TMPDIR="$(mktemp -d)"
cleanup_tmp() { rm -rf "$TMPDIR" >/dev/null 2>&1 || true; }
trap cleanup_tmp EXIT

cat > "${TMPDIR}/Dockerfile" <<'DOCKERFILE'
FROM node:20-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash ca-certificates curl git jq openssh-client tini \
    build-essential pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

# Enable pnpm via corepack (included in Node 20+)
RUN corepack enable pnpm

# Latest CLIs
RUN npm install -g @openai/codex @anthropic-ai/claude-code

# Install Rust (system-wide via rustup)
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path \
  && chmod -R a+rX /usr/local/rustup /usr/local/cargo \
  && echo 'export PATH="/usr/local/cargo/bin:$PATH"' > /etc/profile.d/rust.sh

# Create 'agent' with a free UID >= 1000 (avoid collisions with base image users)
RUN set -eux; \
    if id -u agent >/dev/null 2>&1; then \
      echo "agent user already exists"; \
    else \
      uid=1000; \
      while getent passwd "$uid" >/dev/null 2>&1; do uid=$((uid+1)); done; \
      useradd -m -u "$uid" -s /bin/bash agent; \
    fi

ENV HOME=/home/agent
ENV PATH=/home/agent/.local/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin
ENV TERM=xterm-256color
# Redirect Cargo registry/cache to writable home (toolchain binaries stay at /usr/local/cargo/bin)
ENV CARGO_HOME=/home/agent/.cargo
WORKDIR /work

# swarm wrapper for convenience
RUN cat > /usr/local/bin/swarm <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail

if [[ -x /opt/swarm-hug/target/debug/swarm ]]; then
  exec /opt/swarm-hug/target/debug/swarm "$@"
fi
if [[ -x /opt/swarm-hug/target/release/swarm ]]; then
  exec /opt/swarm-hug/target/release/swarm "$@"
fi

if [[ -f /opt/swarm-hug/Cargo.toml ]]; then
  if [[ "${SWARM_AUTO_BUILD:-1}" == "1" ]]; then
    echo "[swarm] Building debug binary in /opt/swarm-hug..." >&2
    (cd /opt/swarm-hug && cargo build --quiet)
    if [[ -x /opt/swarm-hug/target/debug/swarm ]]; then
      exec /opt/swarm-hug/target/debug/swarm "$@"
    fi
  fi
fi

cat >&2 <<'MSG'
swarm binary not found.
Build it in the repo and try again:
  cd /opt/swarm-hug
  cargo build
MSG
exit 1
WRAP
RUN chmod +x /usr/local/bin/swarm

ENTRYPOINT ["/usr/bin/tini","--"]
CMD ["bash","-l"]
DOCKERFILE

echo "[+] Building image: $IMAGE"
docker --context "$CTX" build -t "$IMAGE" "$TMPDIR"

# Discover agent UID inside the built image
AGENT_UID="$(docker --context "$CTX" run --rm "$IMAGE" bash -lc 'id -u agent')"
AGENT_GID="$(docker --context "$CTX" run --rm "$IMAGE" bash -lc 'id -g agent')"
[[ "$AGENT_UID" =~ ^[0-9]+$ ]] || die "Could not determine agent UID"
[[ "$AGENT_GID" =~ ^[0-9]+$ ]] || die "Could not determine agent GID"
echo "[+] agent uid:gid = ${AGENT_UID}:${AGENT_GID}"

# --- Persistent home volume for auth tokens -----------------------------------
VOL="${VM_NAME}-agent-home"
docker --context "$CTX" volume inspect "$VOL" >/dev/null 2>&1 || {
  echo "[+] Creating volume: $VOL"
  docker --context "$CTX" volume create "$VOL" >/dev/null
}

# Initialize volume so /home/agent/.local/bin exists and is owned by agent UID
# Also set up aliases for swarm
echo "[+] Initializing agent home volume"
docker --context "$CTX" run --rm --user 0:0 \
  -v "${VOL}:/home/agent" \
  "$IMAGE" bash -lc "
    set -e
    mkdir -p /home/agent/.local/bin /home/agent/.config /home/agent/.cache /home/agent/.cargo
    chown -R ${AGENT_UID}:${AGENT_GID} /home/agent
    touch /home/agent/.bashrc
    grep -qE '^[[:space:]]*alias[[:space:]]+swarm=' /home/agent/.bashrc || \
      echo 'alias swarm=/usr/local/bin/swarm' >> /home/agent/.bashrc
    grep -qE '^[[:space:]]*alias[[:space:]]+rebuild-swarm=' /home/agent/.bashrc || \
      echo 'alias rebuild-swarm=\"(cd /opt/swarm-hug && cargo build && echo \\\"[rebuild-swarm] Build complete. Binary at /opt/swarm-hug/target/debug/swarm\\\")\"' >> /home/agent/.bashrc
  " >/dev/null

# --- Start container -----------------------------------------------------------
if docker --context "$CTX" ps -a --format '{{.Names}}' | grep -qx "$CONTAINER_NAME"; then
  echo "[+] Removing existing container: $CONTAINER_NAME"
  docker --context "$CTX" rm -f "$CONTAINER_NAME" >/dev/null
fi

run_args=(
  --name "$CONTAINER_NAME"
  --hostname "$CONTAINER_NAME"
  --user "${AGENT_UID}:${AGENT_GID}"
  --workdir /work

  # Harden container
  --cap-drop ALL
  --security-opt no-new-privileges
  --pids-limit 2048
  --read-only
  --tmpfs /tmp:rw,noexec,nosuid,size=1024m
  --tmpfs /run:rw,noexec,nosuid,size=64m
  --tmpfs /var/tmp:rw,noexec,nosuid,size=256m

  # Persistent auth/config
  -v "${VOL}:/home/agent"

  # Mount swarm-hug repo
  -v "${SCRIPT_DIR}:/opt/swarm-hug:rw"
)

# Mount work folders using their original names
for d in "${FOLDERS_ABS[@]}"; do
  name="$(basename "$d")"
  run_args+=( -v "${d}:/work/${name}:rw" )
done

echo "[+] Starting container: $CONTAINER_NAME"
docker --context "$CTX" run -d "${run_args[@]}" "$IMAGE" sleep infinity >/dev/null

# --- Output -------------------------------------------------------------------
SSHCONF="$(limactl ls --format='{{.SSHConfigFile}}' "$VM_NAME" 2>/dev/null || true)"
LIMA_HOST="lima-${VM_NAME}"

echo ""
echo "Swarm sandbox is up."
echo ""
echo "Docker context:"
echo "  docker --context \"$CTX\" ps"
echo "  docker --context \"$CTX\" exec -it \"$CONTAINER_NAME\" bash -l"
echo ""
echo "Inside the container, your folders are:"
for d in "${FOLDERS_ABS[@]}"; do
  name="$(basename "$d")"
  echo "  /work/${name}  ->  ${d}"
done
echo ""
echo "swarm is available as:"
echo "  swarm (wrapper that auto-builds if needed)"
echo "  /opt/swarm-hug/target/debug/swarm (after build)"
echo ""
echo "To manually rebuild:"
echo "  rebuild-swarm (alias)"
echo ""
echo "Host dev-server access from inside the container/VM:"
echo "  Use: http://host.lima.internal:<port>"
echo "  Ports you said you care about: ${HOST_PORTS}"
echo ""
echo "VM access:"
echo "  limactl shell \"$VM_NAME\""
if [[ -n "${SSHCONF}" ]]; then
  echo "  ssh -F \"${SSHCONF}\" \"${LIMA_HOST}\""
fi
echo ""
echo "One-time authentication (run inside the container):"
echo "  codex login"
echo "  claude"
echo ""

if [[ "$DO_AUTH" == "1" ]]; then
  echo "[+] Dropping you into the container now for auth (exit when done)..."
  docker --context "$CTX" exec -it "$CONTAINER_NAME" bash -l
fi
