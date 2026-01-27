# Manual Testing Guide for lima_connect.sh

This document describes manual test scenarios for validating `lima_connect.sh` behavior as specified in the PRD (specs.md).

## Prerequisites

Before running tests, ensure:
- Lima is installed (`brew install lima`)
- Docker CLI is installed (`brew install docker` or Docker Desktop)
- The script is executable: `chmod +x lima_connect.sh`

## Test Scenarios

### Scenario 1: Single VM with Single Container (Auto-Connect)

**Setup:**
```bash
# Start exactly one Lima VM
limactl start default

# Ensure only one container is running in that VM
docker --context lima-default run -d --name test-container alpine sleep infinity
```

**Test:**
```bash
./lima_connect.sh
```

**Expected Behavior:**
- Script should NOT prompt for VM selection (auto-selects the only running VM)
- Script should NOT prompt for container selection (auto-selects the only running container)
- Script should immediately connect to the container with `bash -l`
- User sees shell prompt inside the container

**Relevant Code:**
- VM auto-selection: `lima_connect.sh:142-145` (select_vm returns immediately if count=1)
- Container auto-selection: `lima_connect.sh:282-285` (select_container returns immediately if count=1)

**Cleanup:**
```bash
docker --context lima-default stop test-container
docker --context lima-default rm test-container
```

---

### Scenario 2: Multiple VMs (Prompt for Selection)

**Setup:**
```bash
# Start two Lima VMs
limactl start default
limactl start secondary
```

**Test:**
```bash
./lima_connect.sh
```

**Expected Behavior:**
- Script should display a selection menu listing both VMs
- If `fzf` is installed: interactive fuzzy finder appears
- If `fzf` is NOT installed: bash `select` menu appears:
  ```
  Select a Lima VM:
  1) default
  2) secondary
  VM number:
  ```
- After selection, proceeds to container selection for chosen VM

**Relevant Code:**
- VM selection with fzf: `lima_connect.sh:148-151`
- VM selection with select fallback: `lima_connect.sh:153-162`

**Cleanup:**
```bash
limactl stop secondary
limactl delete secondary
```

---

### Scenario 3: No VMs Running (Error)

**Setup:**
```bash
# Stop all Lima VMs
limactl stop --all
```

**Test:**
```bash
./lima_connect.sh
```

**Expected Behavior:**
- Script exits with error code 1
- Error message displayed:
  ```
  Error: No running Lima VMs found. Start a VM with: limactl start <name>
  ```

**Relevant Code:**
- `lima_connect.sh:83`:
  ```bash
  [[ ${#RUNNING_VMS[@]} -gt 0 ]] || die "No running Lima VMs found. Start a VM with: limactl start <name>"
  ```

---

### Scenario 4: VM with No Containers (Error)

**Setup:**
```bash
# Start a VM but ensure no containers are running
limactl start default
docker --context lima-default stop $(docker --context lima-default ps -q) 2>/dev/null || true
docker --context lima-default rm $(docker --context lima-default ps -aq) 2>/dev/null || true
```

**Test:**
```bash
./lima_connect.sh
```

**Expected Behavior:**
- Script exits with error code 1
- If no containers exist at all, error message:
  ```
  Error: No containers found in VM 'default'.
  Docker context: lima-default
  To see containers: docker --context lima-default ps -a
  To start a container: docker --context lima-default run ...
  ```
- If containers exist but none are running, error message:
  ```
  Error: No running containers found in VM 'default'.
  Docker context: lima-default
  Stopped containers exist. To start one:
    docker --context lima-default start <container_name>
  ```

**Relevant Code:**
- No containers at all: `lima_connect.sh:238-244`
- No running containers: `lima_connect.sh:259-271`

---

## Additional Test Scenarios

### Scenario 5: Invalid VM Name via --vm Flag

**Test:**
```bash
./lima_connect.sh --vm nonexistent
```

**Expected Behavior:**
- Script exits with error code 1
- Error message:
  ```
  Error: VM 'nonexistent' is not running or does not exist.
  Available running VMs:
    - default
  ```

**Relevant Code:**
- `lima_connect.sh:175-182`

---

### Scenario 6: Invalid Container Name via --container Flag

**Setup:**
```bash
limactl start default
docker --context lima-default run -d --name test-container alpine sleep infinity
```

**Test:**
```bash
./lima_connect.sh --container nonexistent
```

**Expected Behavior:**
- Script exits with error code 1
- Error message:
  ```
  Error: Container 'nonexistent' is not running or does not exist in VM 'default'.
  Available running containers:
    - test-container (Up X minutes)
  ```

**Relevant Code:**
- `lima_connect.sh:331-338`

---

### Scenario 7: Custom Shell Override

**Setup:**
```bash
limactl start default
docker --context lima-default run -d --name test-container alpine sleep infinity
```

**Test:**
```bash
./lima_connect.sh --shell /bin/sh
```

**Expected Behavior:**
- Script connects to container using `/bin/sh` instead of `bash -l`
- Works even if container doesn't have bash installed

**Relevant Code:**
- Shell option parsing: `lima_connect.sh:49`
- Shell execution: `lima_connect.sh:348` (uses $OPT_SHELL)

---

### Scenario 8: Docker Context Creation

**Setup:**
```bash
limactl start default
# Remove the docker context if it exists
docker context rm lima-default 2>/dev/null || true
```

**Test:**
```bash
./lima_connect.sh
```

**Expected Behavior:**
- Script should automatically create the `lima-default` docker context
- Connection should proceed normally after context creation

**Relevant Code:**
- `lima_connect.sh:112-122` (context creation logic in ensure_docker_context)

---

### Scenario 9: Missing Docker Context with Bad Socket

**Setup:**
This scenario tests the error handling when Docker context creation fails.

**Expected Error Messages (depending on failure point):**
- Failed to get docker socket:
  ```
  Error: Failed to get docker socket for VM 'default'.
  The VM may not be fully started or Docker may not be configured.
  Try: limactl shell default -- docker info
  ```
- Empty socket path:
  ```
  Error: Empty docker socket path for VM 'default'.
  This usually means Lima is not configured with Docker support.
  Ensure your Lima VM template includes Docker.
  ```
- Context creation failed:
  ```
  Error: Failed to create docker context 'lima-default'.
  Socket path: unix://...
  Check that:
    1. The Lima VM 'default' is running: limactl list
    2. Docker socket exists: ls -la /path/to/socket
    3. You have permission to access the socket
  ```
- Context exists but unusable:
  ```
  Error: Docker context 'lima-default' exists but is not usable.
  The Docker daemon in VM 'default' may not be running.
  Try: limactl shell default -- docker info
  ```

**Relevant Code:**
- `lima_connect.sh:99-130` (ensure_docker_context error handling)

---

## Test Matrix Summary

| Scenario | VMs | Containers | Flags | Expected Result |
|----------|-----|------------|-------|-----------------|
| 1 | 1 running | 1 running | none | Auto-connect |
| 2 | 2+ running | any | none | VM selection prompt |
| 3 | 0 running | N/A | none | Error: no VMs |
| 4a | 1 running | 0 total | none | Error: no containers |
| 4b | 1 running | 1+ stopped | none | Error: no running + hint |
| 5 | any | any | --vm invalid | Error: invalid VM |
| 6 | 1+ running | 1+ running | --container invalid | Error: invalid container |
| 7 | 1 running | 1 running | --shell /bin/sh | Connect with custom shell |
| 8 | 1 running | 1 running | none (no context) | Create context + connect |
| 9 | 1 running | N/A | none (bad socket) | Helpful error message |

---

## Verification Checklist

- [ ] Scenario 1: Single VM + Single Container auto-connects
- [ ] Scenario 2: Multiple VMs shows selection prompt
- [ ] Scenario 3: No VMs shows helpful error
- [ ] Scenario 4a: No containers shows helpful error
- [ ] Scenario 4b: Stopped containers shows hint to start
- [ ] Scenario 5: Invalid --vm flag shows available VMs
- [ ] Scenario 6: Invalid --container flag shows available containers
- [ ] Scenario 7: --shell flag uses custom shell
- [ ] Scenario 8: Docker context auto-created when missing
- [ ] Scenario 9: Bad socket/context shows diagnostic error

---

## Code Coverage Analysis

The script's error handling paths have been verified by code inspection:

### Error Paths Present in Code:
1. **Missing limactl**: `lima_connect.sh:58` - `die "Missing limactl (Lima). Install: brew install lima"`
2. **Missing docker**: `lima_connect.sh:59` - `die "Missing docker CLI. Install Docker Desktop (or: brew install docker)"`
3. **No running VMs**: `lima_connect.sh:83` - `die "No running Lima VMs found..."`
4. **No containers**: `lima_connect.sh:239-243` - Detailed error with context info
5. **No running containers**: `lima_connect.sh:260-270` - Error with hint about stopped containers
6. **Invalid --vm**: `lima_connect.sh:176-181` - Lists available VMs
7. **Invalid --container**: `lima_connect.sh:332-337` - Lists available containers
8. **Docker context issues**: `lima_connect.sh:99-130` - Multiple error conditions with diagnostics

### Features Present:
1. **Auto-select single VM**: `lima_connect.sh:142-145`
2. **Auto-select single container**: `lima_connect.sh:282-285`
3. **fzf integration**: `lima_connect.sh:148-151` (VM), `lima_connect.sh:288-300` (container)
4. **select fallback**: `lima_connect.sh:153-162` (VM), `lima_connect.sh:303-318` (container)
5. **Container status display**: `lima_connect.sh:255-256` - Shows "name (Up X minutes)"
6. **Flag parsing**: `lima_connect.sh:45-55` - --vm, --container, --shell, --help
7. **Docker context auto-create**: `lima_connect.sh:112-121`

All requirements from specs.md Testing section are covered:
- [x] With one Lima VM running one container - should auto-connect
- [x] With multiple VMs - should prompt for selection
- [x] With no VMs running - should show helpful error
- [x] With VM but no containers - should show helpful error
